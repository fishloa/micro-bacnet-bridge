//! Flash-backed configuration persistence.
//!
//! `BridgeConfig` is serialised with `serde_json_core` and stored in the last
//! 4 KiB sector of the 2 MiB flash. A magic number field provides validity
//! detection; if the magic is absent the default config is returned.

use bridge_core::config::BridgeConfig;
use defmt::{error, info, warn};
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;

/// Total flash size (2 MiB on W5500-EVB-Pico-PoE).
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;

/// Size of one flash sector (erase granularity).
const SECTOR_SIZE: usize = 4096;

/// Byte offset from start-of-flash where config is stored (last 4 KiB sector).
const CONFIG_OFFSET: u32 = (FLASH_SIZE - SECTOR_SIZE) as u32;

/// Scratch buffer for JSON encode/decode — fits inside the sector.
const JSON_BUF_SIZE: usize = SECTOR_SIZE;

/// Thin wrapper that owns the flash peripheral and exposes load/save.
pub struct ConfigManager {
    flash: Flash<'static, FLASH, Async, FLASH_SIZE>,
}

impl ConfigManager {
    /// Create a new `ConfigManager` from a flash peripheral.
    pub fn new(flash: Flash<'static, FLASH, Async, FLASH_SIZE>) -> Self {
        Self { flash }
    }

    /// Consume the `ConfigManager` and return the underlying flash peripheral.
    ///
    /// Used by `main` to hand the flash off to the OTA subsystem after the
    /// initial config load is complete.
    pub fn into_flash(self) -> Flash<'static, FLASH, Async, FLASH_SIZE> {
        self.flash
    }

    /// Load `BridgeConfig` from flash. Returns `BridgeConfig::default()` if
    /// the sector is blank or the magic number is invalid.
    pub fn load(&mut self) -> BridgeConfig {
        let mut sector_buf = [0u8; SECTOR_SIZE];
        match self.flash.blocking_read(CONFIG_OFFSET, &mut sector_buf) {
            Ok(()) => {}
            Err(_) => {
                warn!("config: flash read error, using defaults");
                return BridgeConfig::default();
            }
        }

        // Find the null terminator or use the whole sector
        let json_end = sector_buf
            .iter()
            .position(|&b| b == 0xFF || b == 0x00)
            .unwrap_or(SECTOR_SIZE);

        if json_end < 2 {
            info!("config: blank sector, using defaults");
            return BridgeConfig::default();
        }

        match serde_json_core::from_slice::<BridgeConfig>(&sector_buf[..json_end]) {
            Ok((cfg, _)) if cfg.validate() => {
                info!(
                    "config: loaded from flash (device_id={})",
                    cfg.bacnet.device_id
                );
                cfg
            }
            Ok((cfg, _)) => {
                warn!(
                    "config: invalid magic/version (magic={:#x}), using defaults",
                    cfg.magic
                );
                BridgeConfig::default()
            }
            Err(_) => {
                warn!("config: JSON parse error, using defaults");
                BridgeConfig::default()
            }
        }
    }

    /// Serialise `config` to JSON and write it to the last flash sector.
    ///
    /// Erases the sector first, then programs the serialised bytes.
    ///
    /// # Power-loss safety (M4 — KNOWN LIMITATION)
    ///
    /// This implementation is **not** power-loss safe.  The write sequence is:
    ///   1. Erase the config sector (flash returns to all-0xFF).
    ///   2. Program the new config bytes.
    ///
    /// If power is lost between steps 1 and 2 the sector is blank and the device
    /// boots with default settings on next power-up — no data corruption, but the
    /// stored config is lost.
    ///
    /// **Future improvement:** implement a double-buffered config store using two
    /// alternating flash sectors, each tagged with a generation counter.  The write
    /// procedure then becomes:
    ///   1. Erase the *inactive* sector.
    ///   2. Write the new config to the inactive sector with `generation + 1`.
    ///   3. On load, read both sectors; the one with the higher valid generation
    ///      counter wins.  This survives a power loss at any step because at worst
    ///      the inactive sector is blank, and the active sector still holds the
    ///      last good config.
    ///
    /// This requires allocating two 4 KiB sectors at the top of flash and updating
    /// `CONFIG_OFFSET` and `BridgeConfig` to carry the generation counter.
    ///
    /// # XIP stall risk (C3)
    ///
    /// The RP2040 XIP cache is invalidated during flash erase/program operations.
    /// Any Core 1 code that executes from XIP flash during this window will stall
    /// until the flash operation completes, potentially missing RS-485 byte arrivals
    /// and violating MS/TP timing (one bit at 76800 baud = 13 µs; a flash sector
    /// erase takes ~50 ms).
    ///
    /// TODO: Move Core 1 main loop to .time_critical SRAM section — mark
    ///       `core1_entry`, `mstp_poll`, and `mstp_transmit_outbound` with
    ///       `__attribute__((section(".time_critical")))` in core1_entry.c
    ///       (partially done; verify linker script places .time_critical in SRAM).
    /// TODO: Signal Core 1 to enter an SRAM-only pause loop before calling
    ///       `blocking_erase` / `blocking_write` here, then release it after.
    ///       Use a shared atomic flag (e.g. in the IPC control struct) that
    ///       Core 1 polls between MS/TP frames.
    #[allow(dead_code)]
    pub fn save(&mut self, config: &BridgeConfig) {
        let mut json_buf = [0u8; JSON_BUF_SIZE];
        let json_len = match serde_json_core::to_slice(config, &mut json_buf) {
            Ok(n) => n,
            Err(_) => {
                error!("config: JSON serialise error, not saving");
                return;
            }
        };

        // Erase the sector
        if let Err(_) = self
            .flash
            .blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + SECTOR_SIZE as u32)
        {
            error!("config: flash erase error");
            return;
        }

        // Program the serialised JSON (must be page-aligned in length for some flash controllers;
        // we use the full json_buf padded with 0xFF which is the erased state)
        // Round up to next 256-byte page boundary
        let aligned_len = (json_len + 255) & !255;
        if let Err(_) = self
            .flash
            .blocking_write(CONFIG_OFFSET, &json_buf[..aligned_len])
        {
            error!("config: flash write error");
            return;
        }

        info!("config: saved {} bytes to flash", json_len);
    }
}
