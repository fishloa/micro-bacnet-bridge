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
use bridge_core::ota::{is_uf2, validate_firmware_image, MAX_FIRMWARE_SIZE};
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
                return Err("UF2 not supported; upload raw binary");
            }
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                return Err("Invalid firmware: bad ARM vector table");
            }
            info!(
                "ota: valid image, writing {} sectors to staging",
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
