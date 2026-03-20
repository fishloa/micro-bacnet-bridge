//! HTTP-based OTA firmware update handler.
//!
//! Accepts either a raw ARM binary OR a UF2 file via `POST /api/v1/system/firmware`.
//! The format is auto-detected from the first 4 bytes (UF2 magic vs ARM vector table).
//! The binary is written directly over the running firmware in flash, then
//! the device reboots via `SCB::sys_reset()`.
//!
//! # Safety / limitations
//!
//! This implementation is **not** power-loss safe.  If power is lost while
//! erasing or writing flash the device will boot into a partially-written
//! image and will not start.  A future revision should implement A/B
//! partitioning: write the new image to the second half of flash, verify it,
//! then copy it over (or use the RP2350A bootrom's `REBOOT_TO_ADDR` facility).
//!
//! # Flash layout
//!
//! The RP2350A maps its 2 MB QSPI flash at XIP address `0x10000000`.  The
//! first 256 bytes (`0x10000000`–`0x100000FF`) are the second-stage
//! bootloader (boot2), which must survive the update.  Firmware begins at
//! `0x10000100`; we start erasing/writing there.
//!
//! The last 4 KB sector is reserved for the config store — we must not
//! overwrite it.  [`MAX_IMAGE_SECTORS`] is sized to leave that sector intact.
//!
//! # Core 1 stall
//!
//! Flash erase/write operations invalidate the XIP cache.  Any Core 1 code
//! that executes from flash will stall until the operation completes.  This
//! can violate MS/TP byte-timing (one byte at 76800 baud = ~130 µs; at
//! 9600 baud = ~1.04 ms; a sector erase takes ~50 ms).
//!
//! Staging writes use embassy's `blocking_write` which handles multicore
//! safety internally.  No manual Core 1 pause is needed for staging (upper
//! flash).  After staging completes the connection is closed; the copy from
//! staging to the firmware slot and the subsequent `rom_reboot` with
//! `FLASH_UPDATE` type handle the rest.

use crate::platform::{FIRMWARE_OFFSET, FLASH_SIZE, PROTECTED_OFFSET, SECTOR_SIZE, STAGING_OFFSET};
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

/// Number of flash sectors available for firmware (everything between
/// FIRMWARE_OFFSET and PROTECTED_OFFSET).
/// We write from `FIRMWARE_OFFSET` up to `PROTECTED_OFFSET - 1`.
const MAX_IMAGE_SECTORS: usize =
    (PROTECTED_OFFSET as usize - FIRMWARE_OFFSET as usize) / SECTOR_SIZE;

/// Maximum image size we will accept (must not overrun the config sector).
const MAX_SAFE_IMAGE: usize = MAX_IMAGE_SECTORS * SECTOR_SIZE;

// Sanity: MAX_FIRMWARE_SIZE from bridge-core must fit within our flash layout.
const _: () = assert!(MAX_FIRMWARE_SIZE <= MAX_SAFE_IMAGE);

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
/// Two-phase approach:
///   1. Stream the upload into a staging area in the upper half of flash.
///      Flash writes briefly pause the system (~50ms per sector) but the
///      TCP connection stays alive between sectors.
///   2. After the full image is staged and validated, copy it from staging
///      to the firmware slot (offset 0) in one batch. The connection is
///      already closed by this point so TCP timeouts don't matter.
///
/// Returns `Ok(())` on success; `Err(message)` on failure.
/// The caller is responsible for sending the HTTP response and rebooting.
pub async fn handle_firmware_stream<R: PicoRead>(
    reader: &mut R,
    content_length: usize,
) -> Result<(), &'static str> {
    if content_length > MAX_FIRMWARE_SIZE {
        return Err("Firmware image exceeds maximum allowed size");
    }

    let num_sectors = (content_length + SECTOR_SIZE - 1) / SECTOR_SIZE;
    let mut sector_buf = [0u8; SECTOR_SIZE];
    let mut bytes_received: usize = 0;
    let mut first_sector = true;

    // ---- Phase 0: Pre-erase staging area ----
    info!("ota: pre-erasing staging area ({} sectors)", num_sectors);
    {
        let mut fg = FLASH.lock().await;
        if let Some(flash) = fg.as_mut() {
            let erase_end = STAGING_OFFSET + (num_sectors * SECTOR_SIZE) as u32;
            if flash.blocking_erase(STAGING_OFFSET, erase_end).is_err() {
                return Err("Failed to erase staging area");
            }
        } else {
            return Err("Flash not initialised");
        }
    }
    info!("ota: staging area erased");

    // ---- Phase 1: Stream upload → write pages to staging ----
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
                // UF2 detected — switch to UF2 processing mode.
                // Re-read the rest of the upload as UF2 blocks and write
                // payloads to the staging area at their target addresses.
                info!("ota: UF2 format detected");
                return handle_uf2_stream(
                    reader,
                    content_length,
                    &sector_buf[..sector_data_len],
                    bytes_received,
                )
                .await;
            }
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                return Err("Invalid firmware: bad ARM vector table");
            }
            info!(
                "ota: raw binary, writing {} sectors to staging",
                num_sectors
            );
        }

        // Write page to staging (already erased). No custom Core 1 pause needed —
        // staging area doesn't overlap running code. Embassy's in_ram() handles
        // the brief multicore pause for the write operation.
        let staging_addr = STAGING_OFFSET + (sector_idx * SECTOR_SIZE) as u32;
        let flash_ok = {
            let mut fg = FLASH.lock().await;
            match fg.as_mut() {
                None => false,
                Some(flash) => {
                    let aligned = (sector_data_len + 255) & !255;
                    flash
                        .blocking_write(staging_addr, &sector_buf[..aligned])
                        .is_ok()
                }
            }
        };
        if !flash_ok {
            return Err("Flash write error during staging");
        }
    }

    info!("ota: {} bytes staged successfully", bytes_received);
    Ok(())
}

/// Handle a UF2 upload stream. Called when the first sector contains UF2 magic.
///
/// `first_chunk` contains the first bytes already read (up to one sector).
/// `already_received` is how many bytes have been read so far.
async fn handle_uf2_stream<R: PicoRead>(
    reader: &mut R,
    content_length: usize,
    first_chunk: &[u8],
    already_received: usize,
) -> Result<(), &'static str> {
    const XIP_BASE: u32 = 0x10000000;

    // UF2 files contain 512-byte blocks. Each block has a 256-byte payload
    // and a target address. We parse blocks and write payloads to staging.
    let total_blocks = content_length / UF2_BLOCK_SIZE;
    info!("ota: UF2 upload, {} blocks expected", total_blocks);

    // Track the highest staging address written so we know how much to erase.
    // We pre-erase in chunks as needed rather than the whole staging area upfront,
    // since we don't know the image size from UF2 headers until we see the blocks.
    let mut highest_erased_sector: i32 = -1;
    let mut blocks_written = 0u32;

    // Buffer for accumulating one UF2 block (512 bytes)
    let mut block_buf = [0u8; UF2_BLOCK_SIZE];
    #[allow(unused_assignments)]
    let mut block_cursor = 0usize;

    // Seed with first_chunk data
    let mut remaining = content_length - already_received;
    let first_len = first_chunk.len().min(UF2_BLOCK_SIZE);
    block_buf[..first_len].copy_from_slice(&first_chunk[..first_len]);
    block_cursor = first_len;

    // If first_chunk had more data, we'd need to handle overflow, but
    // sector_buf is 4KB and UF2 blocks are 512 bytes so first_chunk may
    // contain multiple blocks.
    let mut extra_cursor = first_len;

    loop {
        // Process complete blocks from the current buffer
        while block_cursor >= UF2_BLOCK_SIZE {
            let block = &block_buf[..UF2_BLOCK_SIZE];
            match parse_uf2_block(block) {
                Ok((target_addr, payload, _block_num, _total)) => {
                    // Convert XIP address to staging offset
                    if target_addr < XIP_BASE {
                        return Err("UF2 block has invalid target address");
                    }
                    let flash_offset = target_addr - XIP_BASE;
                    let staging_addr = STAGING_OFFSET + flash_offset;

                    if staging_addr + payload.len() as u32 > PROTECTED_OFFSET {
                        return Err("UF2 block would overwrite config area");
                    }

                    // Erase the sector if not already erased
                    let sector_num = (staging_addr / SECTOR_SIZE as u32) as i32;
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

                    // Write payload (256 bytes, already page-aligned)
                    let mut fg = FLASH.lock().await;
                    if let Some(flash) = fg.as_mut() {
                        if flash.blocking_write(staging_addr, payload).is_err() {
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

            // Shift remaining data in block_buf
            let leftover = block_cursor - UF2_BLOCK_SIZE;
            block_buf.copy_within(UF2_BLOCK_SIZE..block_cursor, 0);
            block_cursor = leftover;
        }

        // Process any remaining bytes from first_chunk
        if extra_cursor < first_chunk.len() {
            let copy_len = (first_chunk.len() - extra_cursor).min(UF2_BLOCK_SIZE - block_cursor);
            block_buf[block_cursor..block_cursor + copy_len]
                .copy_from_slice(&first_chunk[extra_cursor..extra_cursor + copy_len]);
            block_cursor += copy_len;
            extra_cursor += copy_len;
            continue;
        }

        // Read more data from the network
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
        "ota: UF2 complete, {} blocks written to staging",
        blocks_written
    );
    Ok(())
}
