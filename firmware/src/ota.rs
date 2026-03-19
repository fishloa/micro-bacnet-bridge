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
//! The RP2350A maps its 4 MB QSPI flash at XIP address `0x10000000`.  The
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

use crate::platform::{FIRMWARE_OFFSET, FLASH_SIZE, PROTECTED_OFFSET, SECTOR_SIZE, STAGING_OFFSET};
use bridge_core::ota::{
    is_uf2, parse_uf2_block, validate_firmware_image, MAX_FIRMWARE_SIZE, UF2_BLOCK_SIZE,
};
use defmt::{error, info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH as FlashPeripheral;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_io_async::Write as _;
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

    // Check staging area has room (staging to config region)
    let staging_limit = (PROTECTED_OFFSET - STAGING_OFFSET) as usize;
    if content_length > staging_limit {
        return Err("Firmware image exceeds staging area");
    }

    let num_sectors = (content_length + SECTOR_SIZE - 1) / SECTOR_SIZE;
    let mut sector_buf = [0u8; SECTOR_SIZE];
    let mut bytes_received: usize = 0;
    let mut first_sector = true;

    // NOTE: Core 1 pause is NOT applied during phase 1. The staging area is in
    // the upper half of flash, which doesn't overlap with running code. Embassy's
    // in_ram() will briefly pause Core 1 via FIFO for each sector, but Core 1's
    // SIO_IRQ_FIFO ISR is in flash and the running code section isn't being erased,
    // so it should be safe. The pause is applied in phase 2 only.

    // ---- Phase 1: Stream upload into staging area ----
    info!(
        "ota: phase 1: receiving {} bytes into staging area",
        content_length
    );

    for sector_idx in 0..num_sectors {
        let sector_start = sector_idx * SECTOR_SIZE;
        let sector_end = (sector_start + SECTOR_SIZE).min(content_length);
        let sector_data_len = sector_end - sector_start;

        sector_buf[..SECTOR_SIZE].fill(0xFF); // erased state

        let mut filled = 0usize;
        while filled < sector_data_len {
            match reader.read(&mut sector_buf[filled..sector_data_len]).await {
                Ok(0) => {
                    warn!(
                        "ota: stream closed early ({}/{})",
                        bytes_received, content_length
                    );
                    return Err("Connection closed before image was fully received");
                }
                Ok(n) => {
                    filled += n;
                    bytes_received += n;
                }
                Err(_) => {
                    warn!("ota: read error at byte {}", bytes_received);
                    return Err("Socket read error during upload");
                }
            }
        }

        // Validate first sector
        if first_sector {
            first_sector = false;
            if is_uf2(&sector_buf) {
                return Err("UF2 upload not supported; upload raw binary");
            }
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                return Err("Invalid firmware: bad ARM vector table");
            }
            info!("ota: valid image, staging {} sectors", num_sectors);
        }

        // Write to staging area
        let staging_addr = STAGING_OFFSET + (sector_idx * SECTOR_SIZE) as u32;
        let flash_ok = {
            let mut fg = FLASH.lock().await;
            match fg.as_mut() {
                None => false,
                Some(flash) => {
                    let ok = flash
                        .blocking_erase(staging_addr, staging_addr + SECTOR_SIZE as u32)
                        .is_ok();
                    if ok {
                        let aligned = (sector_data_len + 255) & !255;
                        flash
                            .blocking_write(staging_addr, &sector_buf[..aligned])
                            .is_ok()
                    } else {
                        false
                    }
                }
            }
        };
        if !flash_ok {
            return Err("Flash write error during staging");
        }
    }

    info!("ota: phase 1 complete: {} bytes staged", bytes_received);

    // ---- Phase 2: Copy from staging to firmware slot ----
    // This overwrites the running firmware. Pause Core 1 first because we're
    // erasing the lower flash region where code runs. Core 1's SIO_IRQ_FIFO
    // ISR is in flash and would fault if that region is being erased.
    info!(
        "ota: phase 2: copying {} sectors to firmware slot",
        num_sectors
    );
    let _pause = crate::core1::pause_core1_for_flash();

    {
        let mut fg = FLASH.lock().await;
        if let Some(flash) = fg.as_mut() {
            for sector_idx in 0..num_sectors {
                let src = STAGING_OFFSET + (sector_idx * SECTOR_SIZE) as u32;
                let dst = FIRMWARE_OFFSET + (sector_idx * SECTOR_SIZE) as u32;

                // Read from staging
                if flash.blocking_read(src, &mut sector_buf).is_err() {
                    error!("ota: read-back failed at staging {:#x}", src);
                    return Err("Flash read error during copy");
                }

                // Erase destination
                if flash.blocking_erase(dst, dst + SECTOR_SIZE as u32).is_err() {
                    error!("ota: erase failed at {:#x}", dst);
                    return Err("Flash erase error — device may need recovery via probe");
                }

                // Write to destination
                if flash.blocking_write(dst, &sector_buf).is_err() {
                    error!("ota: write failed at {:#x}", dst);
                    return Err("Flash write error — device may need recovery via probe");
                }
            }
        } else {
            return Err("Flash not initialised");
        }
    }

    info!("ota: phase 2 complete — firmware updated successfully");
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy TcpSocket OTA entry point (kept for reference; not called by picoserve path)
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
#[allow(dead_code)]
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

    // embassy-rp's flash.blocking_erase() calls multicore::pause_core1() internally,
    // so no manual Core 1 pause is needed here.

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

        // ---- 3. Detect format + validate on first sector ----
        if first_sector {
            first_sector = false;
            if is_uf2(&sector_buf) {
                // UF2 detected — switch to UF2 processing mode.
                info!("ota: UF2 format detected, switching to UF2 handler");
                handle_uf2_upload(socket, content_length, &sector_buf[..sector_data_len]).await;
                return;
            }
            // Raw binary — validate ARM vector table
            if !validate_firmware_image(&sector_buf[..8.min(sector_data_len)]) {
                warn!("ota: invalid ARM vector table — rejecting image");
                send_response(socket, 400, "Invalid firmware: bad ARM vector table").await;
                return;
            }
            info!("ota: raw binary, vector table OK, beginning flash write");
        }

        // ---- 4. Erase + write the sector ----
        let flash_offset = FIRMWARE_OFFSET + (sector_idx * SECTOR_SIZE) as u32;

        if flash_offset + SECTOR_SIZE as u32 > PROTECTED_OFFSET {
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
    crate::system_reset();
}

// ---------------------------------------------------------------------------
// UF2 upload handler
// ---------------------------------------------------------------------------

/// Handle a UF2 upload. Called when the first sector is detected as UF2.
/// `first_block_data` contains the first chunk already read (up to 4096 bytes,
/// may contain multiple UF2 blocks).
async fn handle_uf2_upload(
    socket: &mut TcpSocket<'_>,
    content_length: usize,
    first_block_data: &[u8],
) {
    let total_blocks = content_length / UF2_BLOCK_SIZE;
    if total_blocks == 0 {
        send_response(socket, 400, "UF2 file too small").await;
        return;
    }

    // We process UF2 blocks one at a time (512 bytes each).
    // Each block specifies its own target address, so we erase+write per sector as needed.
    let mut block_buf = [0u8; UF2_BLOCK_SIZE];
    let mut blocks_written = 0u32;
    let mut bytes_consumed = 0usize;
    let mut validated = false;

    // Track which sectors we've already erased to avoid double-erase
    let mut last_erased_sector: u32 = u32::MAX;

    // Sector accumulation buffer — we accumulate 256-byte UF2 payloads into
    // a full 4096-byte sector before writing
    let mut sector_buf = [0xFFu8; SECTOR_SIZE]; // 0xFF = erased state
    let mut current_sector_addr: u32 = u32::MAX;
    let mut sector_fill = 0usize;

    // Process data: first from first_block_data, then from socket
    let mut source_cursor = 0usize;
    let mut reading_socket = false;

    loop {
        // Fill block_buf with 512 bytes
        let mut filled = 0usize;

        // Drain from first_block_data first
        if !reading_socket {
            while filled < UF2_BLOCK_SIZE && source_cursor < first_block_data.len() {
                block_buf[filled] = first_block_data[source_cursor];
                filled += 1;
                source_cursor += 1;
            }
            if source_cursor >= first_block_data.len() {
                reading_socket = true;
            }
        }

        // Then from socket
        while filled < UF2_BLOCK_SIZE {
            match socket.read(&mut block_buf[filled..UF2_BLOCK_SIZE]).await {
                Ok(0) => {
                    if filled == 0 && blocks_written > 0 {
                        // Normal end of stream
                        break;
                    }
                    warn!("ota: UF2 socket closed early");
                    send_response(socket, 400, "Connection closed during UF2 upload").await;
                    return;
                }
                Ok(n) => filled += n,
                Err(_) => {
                    send_response(socket, 500, "Socket read error").await;
                    return;
                }
            }
        }

        if filled < UF2_BLOCK_SIZE {
            break; // End of data
        }

        bytes_consumed += UF2_BLOCK_SIZE;

        // Parse the UF2 block
        let (target_addr, payload, _block_num, _total) = match parse_uf2_block(&block_buf) {
            Ok(v) => v,
            Err(e) => {
                warn!("ota: bad UF2 block at byte {}: {}", bytes_consumed, e);
                send_response(socket, 400, "Malformed UF2 block").await;
                return;
            }
        };

        // Validate first block's payload contains a valid vector table
        if !validated {
            // The first UF2 block targets the start of flash. Check vector table.
            if target_addr == 0x10000000 || target_addr == 0x10000100 {
                if payload.len() >= 8 && !validate_firmware_image(&payload[..8]) {
                    send_response(socket, 400, "Invalid firmware: bad ARM vector table in UF2")
                        .await;
                    return;
                }
            }
            validated = true;
            info!("ota: UF2 validated, writing {} blocks", total_blocks);
        }

        // Convert XIP address to flash offset (strip 0x10000000 base)
        let flash_offset = target_addr.wrapping_sub(0x10000000);

        // Check bounds
        if flash_offset + payload.len() as u32 > PROTECTED_OFFSET {
            send_response(socket, 400, "UF2 block targets config sector").await;
            return;
        }

        // Which sector does this block belong to?
        let sector_addr = flash_offset & !(SECTOR_SIZE as u32 - 1);

        // If we've moved to a new sector, flush the old one
        if sector_addr != current_sector_addr && current_sector_addr != u32::MAX {
            if !flush_sector(current_sector_addr, &sector_buf).await {
                send_response(socket, 500, "Flash write error").await;
                return;
            }
            sector_buf = [0xFFu8; SECTOR_SIZE];
            sector_fill = 0;
        }

        current_sector_addr = sector_addr;

        // Copy payload into sector buffer at the correct offset
        let offset_in_sector = (flash_offset - sector_addr) as usize;
        let end = (offset_in_sector + payload.len()).min(SECTOR_SIZE);
        sector_buf[offset_in_sector..end].copy_from_slice(&payload[..end - offset_in_sector]);
        sector_fill = sector_fill.max(end);

        blocks_written += 1;
        let _ = last_erased_sector; // suppress unused warning
        last_erased_sector = sector_addr;

        if bytes_consumed >= content_length {
            break;
        }
    }

    // Flush the last sector
    if current_sector_addr != u32::MAX && sector_fill > 0 {
        if !flush_sector(current_sector_addr, &sector_buf).await {
            send_response(socket, 500, "Flash write error on last sector").await;
            return;
        }
    }

    info!("ota: UF2 complete, {} blocks written", blocks_written);
    send_response(
        socket,
        200,
        "Firmware update complete. Device is rebooting...",
    )
    .await;
    Timer::after_millis(500).await;
    crate::system_reset();
}

/// Erase a sector and write data to it.
async fn flush_sector(sector_offset: u32, data: &[u8; SECTOR_SIZE]) -> bool {
    let mut flash_guard = FLASH.lock().await;
    match flash_guard.as_mut() {
        None => {
            error!("ota: flash not initialised");
            false
        }
        Some(flash) => {
            if flash
                .blocking_erase(sector_offset, sector_offset + SECTOR_SIZE as u32)
                .is_err()
            {
                error!("ota: erase failed at {:#x}", sector_offset);
                return false;
            }
            if flash.blocking_write(sector_offset, data).is_err() {
                error!("ota: write failed at {:#x}", sector_offset);
                return false;
            }
            info!("ota: sector at {:#x} written", sector_offset);
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Content-Length parser
// ---------------------------------------------------------------------------

/// Extract the `Content-Length` value from a raw HTTP request header block.
///
/// Returns `None` if the header is absent or cannot be parsed.
#[allow(dead_code)]
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
