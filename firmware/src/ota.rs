//! HTTP-based OTA firmware update handler.
//!
//! Accepts a raw ARM binary (not UF2) via `POST /api/v1/system/firmware`.
//! The binary is written directly over the running firmware in flash, then
//! the device reboots via `SCB::sys_reset()`.
//!
//! # Safety / limitations
//!
//! This implementation is **not** power-loss safe.  If power is lost while
//! erasing or writing flash the device will boot into a partially-written
//! image and will not start.  A future revision should implement A/B
//! partitioning: write the new image to the second half of flash, verify it,
//! then copy it over (or use the RP2040 bootrom's `REBOOT_TO_ADDR` facility).
//!
//! # Flash layout
//!
//! The RP2040 maps its 2 MB QSPI flash at XIP address `0x10000000`.  The
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
//! can violate MS/TP byte-timing (one byte at 76800 baud = ~104 µs; a sector
//! erase takes ~50 ms).
//!
//! **TODO:** Before beginning the flash write sequence, signal Core 1 to
//! suspend (enter an SRAM-resident busy-wait loop) via the shared IPC atomic
//! flag, then release it after the write is done.  This is tracked in the
//! same TODO block as `config::ConfigManager::save`.

use crate::config::FLASH_SIZE;
use bridge_core::ota::{validate_firmware_image, MAX_FIRMWARE_SIZE};
use defmt::{error, info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH as FlashPeripheral;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_io_async::Write as _;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Byte offset in flash where firmware starts (after the 256-byte boot2 stage).
const FIRMWARE_OFFSET: u32 = 0x100;

/// Size of one flash erase sector (4 KB on RP2040 / RP2350).
const SECTOR_SIZE: usize = 4096;

/// Last flash sector is reserved for config — do not overwrite it.
const CONFIG_SECTOR_OFFSET: u32 = (FLASH_SIZE - SECTOR_SIZE) as u32;

/// Number of flash sectors available for firmware (everything between boot2
/// and the config sector, inclusive of the first sector that contains boot2).
/// We write from `FIRMWARE_OFFSET` up to `CONFIG_SECTOR_OFFSET - 1`.
const MAX_IMAGE_SECTORS: usize =
    (CONFIG_SECTOR_OFFSET as usize - FIRMWARE_OFFSET as usize) / SECTOR_SIZE;

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

/// Handle `POST /api/v1/system/firmware`.
///
/// Reads the binary from the already-accepted TCP socket, validates it,
/// writes it to flash sector-by-sector, then triggers a system reset.
///
/// The caller must have already parsed the HTTP request line and headers and
/// supplied `content_length` from the `Content-Length` header.  `body_start`
/// is the slice of the initial read buffer that follows the `\r\n\r\n`
/// boundary.
pub async fn handle_firmware_upload(
    socket: &mut TcpSocket<'_>,
    content_length: usize,
    body_start: &[u8],
) {
    // ---- 1. Validate declared size ----
    if content_length == 0 {
        warn!("ota: Content-Length is 0");
        send_response(socket, 400, "Content-Length must be non-zero").await;
        return;
    }
    if content_length > MAX_FIRMWARE_SIZE {
        warn!(
            "ota: image too large: {} > {}",
            content_length, MAX_FIRMWARE_SIZE
        );
        send_response(socket, 413, "Firmware image exceeds maximum allowed size").await;
        return;
    }

    // ---- 2. Receive the full image into a per-sector 4 KB window ----
    // We process the image one sector at a time to avoid needing a large
    // contiguous RAM buffer (which would be impossible without heap).
    //
    // Sector buffer lives on the stack — 4 KB is within the 64 KB headroom.
    let mut sector_buf = [0u8; SECTOR_SIZE];
    let mut bytes_received: usize = 0;
    let mut first_sector = true;

    // Copy any bytes that arrived in the same read as the HTTP headers.
    let mut body_cursor = 0usize;

    // We iterate one full sector at a time.
    let num_sectors = (content_length + SECTOR_SIZE - 1) / SECTOR_SIZE;

    for sector_idx in 0..num_sectors {
        let sector_start = sector_idx * SECTOR_SIZE;
        let sector_end = (sector_start + SECTOR_SIZE).min(content_length);
        let sector_data_len = sector_end - sector_start;

        // Fill sector_buf[0..sector_data_len] from body_start + socket.
        let mut filled = 0usize;

        // First, drain whatever is left in body_start.
        while filled < sector_data_len && body_cursor < body_start.len() {
            sector_buf[filled] = body_start[body_cursor];
            filled += 1;
            body_cursor += 1;
            bytes_received += 1;
        }

        // Then read from the socket until the sector window is full.
        while filled < sector_data_len {
            match socket.read(&mut sector_buf[filled..sector_data_len]).await {
                Ok(0) => {
                    warn!(
                        "ota: socket closed early (received {} of {} bytes)",
                        bytes_received, content_length
                    );
                    send_response(
                        socket,
                        400,
                        "Connection closed before image was fully received",
                    )
                    .await;
                    return;
                }
                Ok(n) => {
                    filled += n;
                    bytes_received += n;
                }
                Err(_) => {
                    warn!("ota: socket read error at byte {}", bytes_received);
                    send_response(socket, 500, "Socket read error during upload").await;
                    return;
                }
            }
        }

        // ---- 3. Validate first sector (ARM vector table) ----
        if first_sector {
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                warn!("ota: invalid ARM vector table — rejecting image");
                send_response(socket, 400, "Invalid firmware: bad ARM vector table").await;
                return;
            }
            first_sector = false;
            info!("ota: vector table OK, beginning flash write");
        }

        // ---- 4. Erase + write the sector ----
        let flash_offset = FIRMWARE_OFFSET + (sector_idx * SECTOR_SIZE) as u32;

        if flash_offset + SECTOR_SIZE as u32 > CONFIG_SECTOR_OFFSET {
            // Should never happen given the size check above.
            error!(
                "ota: sector {} would overwrite config sector — aborting",
                sector_idx
            );
            send_response(socket, 500, "Internal error: image would overwrite config").await;
            return;
        }

        // Erase and write the sector under the flash mutex.
        let flash_ok = {
            let mut flash_guard = FLASH.lock().await;
            match flash_guard.as_mut() {
                None => {
                    error!("ota: flash peripheral not initialised");
                    false
                }
                Some(flash) => {
                    // Erase the sector.
                    let erase_ok = flash
                        .blocking_erase(flash_offset, flash_offset + SECTOR_SIZE as u32)
                        .is_ok();
                    if !erase_ok {
                        error!("ota: erase failed at offset {:#x}", flash_offset);
                        false
                    } else {
                        // Pad to page boundary (256 bytes) as required by the
                        // flash controller.  sector_buf bytes beyond
                        // sector_data_len are 0x00 from array initialisation;
                        // this is harmless for unused trailing bytes.
                        let aligned = (sector_data_len + 255) & !255;
                        let write_ok = flash
                            .blocking_write(flash_offset, &sector_buf[..aligned])
                            .is_ok();
                        if !write_ok {
                            error!("ota: write failed at offset {:#x}", flash_offset);
                        }
                        write_ok
                    }
                }
            }
        };

        if !flash_ok {
            send_response(
                socket,
                500,
                "Flash write error — device may be in bad state",
            )
            .await;
            return;
        }

        info!(
            "ota: sector {}/{} written ({} bytes)",
            sector_idx + 1,
            num_sectors,
            sector_data_len,
        );
    }

    info!(
        "ota: {} bytes written to flash successfully",
        bytes_received
    );

    // ---- 5. Send success response before rebooting ----
    send_response(
        socket,
        200,
        "Firmware update complete. Device is rebooting...",
    )
    .await;

    // Allow the TCP stack to drain the response.
    Timer::after_millis(500).await;

    // ---- 6. Reboot ----
    info!("ota: triggering system reset");
    cortex_m::peripheral::SCB::sys_reset();
}

// ---------------------------------------------------------------------------
// Content-Length parser
// ---------------------------------------------------------------------------

/// Extract the `Content-Length` value from a raw HTTP request header block.
///
/// Returns `None` if the header is absent or cannot be parsed.
pub fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = {
            // Compare case-insensitively without heap allocation.
            // We only need to check the prefix "content-length:".
            let bytes = line.as_bytes();
            // Manual ASCII lower-case comparison for the known prefix.
            const PREFIX: &[u8] = b"content-length:";
            if bytes.len() < PREFIX.len() {
                continue;
            }
            let matches = bytes[..PREFIX.len()]
                .iter()
                .zip(PREFIX)
                .all(|(a, b)| a.to_ascii_lowercase() == *b);
            if !matches {
                continue;
            }
            &line[PREFIX.len()..]
        };
        let trimmed = lower.trim();
        if let Ok(n) = trimmed.parse::<usize>() {
            return Some(n);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Response helper (local, not shared with http.rs)
// ---------------------------------------------------------------------------

async fn send_response(socket: &mut TcpSocket<'_>, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let mut hdr: heapless::String<128> = heapless::String::new();
    let _ = core::fmt::write(
        &mut hdr,
        format_args!(
            "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
            status,
            reason,
            body.len()
        ),
    );
    let _ = socket.write_all(hdr.as_bytes()).await;
    let _ = socket.write_all(body.as_bytes()).await;
    let _ = socket.flush().await;
}
