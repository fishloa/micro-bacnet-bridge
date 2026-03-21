//! HTTP-based OTA firmware update handler with A/B slot alternation.
//!
//! Accepts either a raw ARM binary OR a UF2 file via `POST /api/v1/system/firmware`.
//! The format is auto-detected from the first 4 bytes (UF2 magic vs ARM vector table).
//!
//! # A/B slot design
//!
//! Flash is divided into two firmware slots (A and B).  OTA always writes to
//! the INACTIVE slot (determined at runtime by checking which slot the current
//! code is executing from).  After staging completes, the device reboots with
//! `REBOOT_TYPE_FLASH_UPDATE` pointing the bootrom at the newly-written slot.
//! The bootrom remaps the target slot to XIP base address `0x10000000`.
//!
//! This means consecutive OTAs alternate between slots, and a failed update
//! leaves the active slot intact — the device can still boot.
//!
//! # Flash layout (2 MB)
//!
//! ```text
//!   [Slot A:   0x000000–0x0FFFFF]  (1 MB)
//!   [Slot B:   0x100000–0x1EFFFF]  (960 KB)
//!   [Config:   0x1F0000–0x1FBFFF]  (48 KB)
//!   [Identity: 0x1FF000–0x1FFFFF]  (4 KB)
//! ```
//!
//! # Core 1 stall
//!
//! Flash erase/write operations invalidate the XIP cache.  Embassy's
//! `blocking_write` handles multicore safety internally.  Writing to the
//! inactive slot doesn't overlap running code, so no manual Core 1 pause
//! is needed during staging.

use crate::platform::{FLASH_SIZE, PROTECTED_OFFSET, SECTOR_SIZE, SLOT_B_OFFSET, XIP_BASE};
use bridge_core::ota::{
    is_uf2, parse_uf2_block, validate_firmware_image, MAX_FIRMWARE_SIZE, UF2_BLOCK_SIZE,
};
use defmt::info;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH as FlashPeripheral;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use picoserve::io::Read as PicoRead;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum firmware size that fits in either slot.
/// Slot B is the smaller one: PROTECTED_OFFSET - SLOT_B_OFFSET = 960 KB.
#[allow(dead_code)]
const MAX_SLOT_SIZE: usize = (PROTECTED_OFFSET - SLOT_B_OFFSET) as usize;

// Sanity: each slot must have non-zero usable space.
const _: () = assert!((PROTECTED_OFFSET - SLOT_B_OFFSET) as usize > 0);
const _: () = assert!(SLOT_B_OFFSET > 0);

// ---------------------------------------------------------------------------
// Global flash mutex
// ---------------------------------------------------------------------------

/// Global flash peripheral, protected by a mutex so that config saves and OTA
/// writes are serialised.  Initialised by `main` before spawning tasks.
pub static FLASH: Mutex<
    CriticalSectionRawMutex,
    Option<Flash<'static, FlashPeripheral, Async, FLASH_SIZE>>,
> = Mutex::new(None);

// ---------------------------------------------------------------------------
// OTA upload handler
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// picoserve streaming OTA entry point
// ---------------------------------------------------------------------------

/// Handle an OTA firmware upload via a picoserve `RequestBodyReader`.
///
/// Writes the incoming firmware to the INACTIVE flash slot (A/B alternation).
/// The target slot is determined at runtime by [`crate::platform::ota_target_slot()`].
///
/// Returns `Ok(())` on success; `Err(message)` on failure.
/// The caller is responsible for sending the HTTP response and rebooting.
pub async fn handle_firmware_stream<R: PicoRead>(
    reader: &mut R,
    content_length: usize,
) -> Result<(), &'static str> {
    let target_offset = crate::platform::ota_target_slot();
    let max_size = if target_offset == SLOT_B_OFFSET {
        (PROTECTED_OFFSET - SLOT_B_OFFSET) as usize
    } else {
        SLOT_B_OFFSET as usize
    };

    if content_length > max_size {
        return Err("Firmware image exceeds slot size");
    }
    if content_length > MAX_FIRMWARE_SIZE {
        return Err("Firmware image exceeds maximum allowed size");
    }

    info!(
        "ota: target slot at offset {:#x}, {} bytes",
        target_offset, content_length
    );

    let num_sectors = (content_length + SECTOR_SIZE - 1) / SECTOR_SIZE;
    let mut sector_buf = [0u8; SECTOR_SIZE];
    let mut bytes_received: usize = 0;
    let mut first_sector = true;

    // ---- Phase 0: Pre-erase target slot ----
    info!("ota: pre-erasing {} sectors", num_sectors);
    {
        let mut fg = FLASH.lock().await;
        if let Some(flash) = fg.as_mut() {
            let erase_start = target_offset;
            let erase_end = target_offset + (num_sectors * SECTOR_SIZE) as u32;
            if flash.blocking_erase(erase_start, erase_end).is_err() {
                return Err("Failed to erase target slot");
            }
        } else {
            return Err("Flash not initialised");
        }
    }
    info!("ota: target slot erased");

    // ---- Phase 1: Stream upload → write pages to target slot ----
    info!("ota: receiving {} bytes", content_length);
    for sector_idx in 0..num_sectors {
        let sector_end = ((sector_idx + 1) * SECTOR_SIZE).min(content_length);
        let sector_data_len = sector_end - sector_idx * SECTOR_SIZE;

        sector_buf[..SECTOR_SIZE].fill(0xFF);
        let mut filled = 0usize;
        while filled < sector_data_len {
            match reader.read(&mut sector_buf[filled..sector_data_len]).await {
                Ok(0) => return Err("Connection closed before image fully received"),
                Ok(n) => {
                    filled += n;
                    bytes_received += n;
                }
                Err(_) => return Err("Socket read error"),
            }
        }

        if first_sector {
            first_sector = false;
            if is_uf2(&sector_buf) {
                info!("ota: UF2 format detected");
                return handle_uf2_stream(
                    reader,
                    content_length,
                    &sector_buf[..sector_data_len],
                    bytes_received,
                    target_offset,
                )
                .await;
            }
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                return Err("Invalid firmware: bad ARM vector table");
            }
            info!(
                "ota: raw binary, writing {} sectors to slot at {:#x}",
                num_sectors, target_offset
            );
        }

        let write_addr = target_offset + (sector_idx * SECTOR_SIZE) as u32;
        let flash_ok = {
            let mut fg = FLASH.lock().await;
            match fg.as_mut() {
                None => false,
                Some(flash) => {
                    let aligned = (sector_data_len + 255) & !255;
                    flash
                        .blocking_write(write_addr, &sector_buf[..aligned])
                        .is_ok()
                }
            }
        };
        if !flash_ok {
            return Err("Flash write error");
        }
    }

    info!(
        "ota: {} bytes staged at {:#x}",
        bytes_received, target_offset
    );
    Ok(())
}

/// Handle a UF2 upload stream. Called when the first sector contains UF2 magic.
///
/// `first_chunk` contains the first bytes already read (up to one sector).
/// `already_received` is how many bytes have been read so far.
/// `target_offset` is the flash offset of the inactive slot.
async fn handle_uf2_stream<R: PicoRead>(
    reader: &mut R,
    content_length: usize,
    first_chunk: &[u8],
    already_received: usize,
    target_offset: u32,
) -> Result<(), &'static str> {
    let total_blocks = content_length / UF2_BLOCK_SIZE;
    info!("ota: UF2 upload, {} blocks expected", total_blocks);

    let mut highest_erased_sector: i32 = -1;
    let mut blocks_written = 0u32;

    let mut block_buf = [0u8; UF2_BLOCK_SIZE];
    #[allow(unused_assignments)]
    let mut block_cursor = 0usize;

    let mut remaining = content_length - already_received;
    let first_len = first_chunk.len().min(UF2_BLOCK_SIZE);
    block_buf[..first_len].copy_from_slice(&first_chunk[..first_len]);
    block_cursor = first_len;

    let mut extra_cursor = first_len;

    loop {
        while block_cursor >= UF2_BLOCK_SIZE {
            let block = &block_buf[..UF2_BLOCK_SIZE];
            match parse_uf2_block(block) {
                Ok((target_addr, payload, _block_num, _total)) => {
                    if target_addr < XIP_BASE {
                        return Err("UF2 block has invalid target address");
                    }
                    let flash_offset = target_addr - XIP_BASE;
                    let write_addr = target_offset + flash_offset;

                    if write_addr + payload.len() as u32 > PROTECTED_OFFSET {
                        return Err("UF2 block would overwrite config area");
                    }

                    // Erase the sector if not already erased
                    let sector_num = (write_addr / SECTOR_SIZE as u32) as i32;
                    if sector_num > highest_erased_sector {
                        let erase_addr = sector_num as u32 * SECTOR_SIZE as u32;
                        let mut fg = FLASH.lock().await;
                        if let Some(flash) = fg.as_mut() {
                            if flash
                                .blocking_erase(erase_addr, erase_addr + SECTOR_SIZE as u32)
                                .is_err()
                            {
                                return Err("Flash erase error during UF2 staging");
                            }
                        }
                        highest_erased_sector = sector_num;
                    }

                    let mut fg = FLASH.lock().await;
                    if let Some(flash) = fg.as_mut() {
                        if flash.blocking_write(write_addr, payload).is_err() {
                            return Err("Flash write error during UF2 staging");
                        }
                    }
                    blocks_written += 1;
                }
                Err(e) => {
                    info!("ota: UF2 parse error: {}", e);
                    return Err("Invalid UF2 block");
                }
            }

            let leftover = block_cursor - UF2_BLOCK_SIZE;
            block_buf.copy_within(UF2_BLOCK_SIZE..block_cursor, 0);
            block_cursor = leftover;
        }

        if extra_cursor < first_chunk.len() {
            let copy_len = (first_chunk.len() - extra_cursor).min(UF2_BLOCK_SIZE - block_cursor);
            block_buf[block_cursor..block_cursor + copy_len]
                .copy_from_slice(&first_chunk[extra_cursor..extra_cursor + copy_len]);
            block_cursor += copy_len;
            extra_cursor += copy_len;
            continue;
        }

        if remaining == 0 {
            break;
        }
        let to_read = remaining.min(UF2_BLOCK_SIZE - block_cursor);
        match reader
            .read(&mut block_buf[block_cursor..block_cursor + to_read])
            .await
        {
            Ok(0) => break,
            Ok(n) => {
                block_cursor += n;
                remaining -= n;
            }
            Err(_) => return Err("Socket read error during UF2 upload"),
        }
    }

    info!(
        "ota: UF2 complete, {} blocks written to slot at {:#x}",
        blocks_written, target_offset
    );
    Ok(())
}
