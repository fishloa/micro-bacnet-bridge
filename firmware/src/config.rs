//! Flash-backed configuration persistence.
//!
//! `BridgeConfig` is serialised with `serde_json_core` and stored in the last
//! 4 KiB sector of the 2 MiB flash. A magic number field provides validity
//! detection; if the magic is absent the default config is returned.

use bridge_core::config::{BridgeConfig, MAGIC};
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
#[allow(dead_code)]
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

    /// Load `BridgeConfig` from flash. Returns `BridgeConfig::default()` if
    /// the sector is blank or the magic number is invalid.
    pub fn load(&mut self) -> BridgeConfig {
        let mut sector_buf = [0u8; SECTOR_SIZE];
        match self
            .flash
            .blocking_read(CONFIG_OFFSET, &mut sector_buf)
        {
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
            Ok((cfg, _)) if cfg.magic == MAGIC && cfg.validate() => {
                info!("config: loaded from flash (device_id={})", cfg.bacnet.device_id);
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
        if let Err(_) = self.flash.blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + SECTOR_SIZE as u32) {
            error!("config: flash erase error");
            return;
        }

        // Program the serialised JSON (must be page-aligned in length for some flash controllers;
        // we use the full json_buf padded with 0xFF which is the erased state)
        // Round up to next 256-byte page boundary
        let aligned_len = (json_len + 255) & !255;
        if let Err(_) = self.flash.blocking_write(CONFIG_OFFSET, &json_buf[..aligned_len]) {
            error!("config: flash write error");
            return;
        }

        info!("config: saved {} bytes to flash", json_len);
    }
}
