//! Flash-backed configuration persistence.
//!
//! `BridgeConfig` is serialised with `serde_json_core` and stored in a 32 KiB
//! config region at `CONFIG_OFFSET` (top-of-flash − 64 KiB). A magic number
//! and version field provide validity detection; if the magic is absent the
//! default config is returned.

use crate::platform::{CONFIG_OFFSET, CONFIG_SIZE, FLASH_SIZE, IDENTITY_OFFSET, SECTOR_SIZE};
use bridge_core::config::BridgeConfig;
use defmt::{error, info, warn};
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;

/// Identity sector magic bytes: "IDNT".
const IDENTITY_MAGIC: [u8; 4] = [0x49, 0x44, 0x4E, 0x54];

/// Scratch buffer for JSON encode/decode.
/// 8 KiB is enough for the expanded v4 config including all point rules.
const JSON_BUF_SIZE: usize = 8192;

/// Thin wrapper that owns the flash peripheral and exposes load/save.
pub struct ConfigManager {
    flash: Flash<'static, FLASH, Async, FLASH_SIZE>,
}

impl ConfigManager {
    /// Create a new `ConfigManager` from a flash peripheral.
    pub fn new(flash: Flash<'static, FLASH, Async, FLASH_SIZE>) -> Self {
        Self { flash }
    }

    /// Load MAC address from the identity sector. Returns None if not yet written.
    pub fn load_mac(&mut self) -> Option<[u8; 6]> {
        let mut buf = [0u8; 16];
        if self.flash.blocking_read(IDENTITY_OFFSET, &mut buf).is_err() {
            return None;
        }
        if buf[0..4] != IDENTITY_MAGIC {
            return None;
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&buf[4..10]);
        // All zeros means not written
        if mac == [0u8; 6] {
            return None;
        }
        info!(
            "identity: loaded MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
        Some(mac)
    }

    /// Erase the identity sector (to allow MAC regeneration). Used by factory reset.
    #[allow(dead_code)]
    pub fn erase_identity(&mut self) {
        let _ = self
            .flash
            .blocking_erase(IDENTITY_OFFSET, IDENTITY_OFFSET + SECTOR_SIZE as u32);
        info!("identity: sector erased");
    }

    /// Save MAC address to the identity sector.
    pub fn save_mac(&mut self, mac: &[u8; 6]) {
        // Write identity sector: [magic(4)] [mac(6)] [padding(rest)]
        let mut sector = [0xFFu8; SECTOR_SIZE];
        sector[0..4].copy_from_slice(&IDENTITY_MAGIC);
        sector[4..10].copy_from_slice(mac);

        if self
            .flash
            .blocking_erase(IDENTITY_OFFSET, IDENTITY_OFFSET + SECTOR_SIZE as u32)
            .is_err()
        {
            error!("identity: flash erase error");
            return;
        }
        // Write first 256 bytes (page-aligned)
        if self
            .flash
            .blocking_write(IDENTITY_OFFSET, &sector[..256])
            .is_err()
        {
            error!("identity: flash write error");
            return;
        }
        info!(
            "identity: saved MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    }

    /// Consume the `ConfigManager` and return the underlying flash peripheral.
    ///
    /// Used by `main` to hand the flash off to the OTA subsystem after the
    /// initial config load is complete.
    pub fn into_flash(self) -> Flash<'static, FLASH, Async, FLASH_SIZE> {
        self.flash
    }

    /// Load `BridgeConfig` from flash. Returns `BridgeConfig::default()` if
    /// the config region is blank or the magic number is invalid.
    pub fn load(&mut self) -> BridgeConfig {
        let mut sector_buf = [0u8; JSON_BUF_SIZE];
        match self.flash.blocking_read(CONFIG_OFFSET, &mut sector_buf) {
            Ok(()) => {}
            Err(_) => {
                warn!("config: flash read error, using defaults");
                return BridgeConfig::default();
            }
        }

        // Find the null terminator or use the whole buffer.
        let json_end = sector_buf
            .iter()
            .position(|&b| b == 0xFF || b == 0x00)
            .unwrap_or(JSON_BUF_SIZE);

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
    /// The RP2350A XIP cache is invalidated during flash erase/program operations.
    /// Any Core 1 code that executes from XIP flash during this window will stall
    /// until the flash operation completes, potentially missing RS-485 byte arrivals
    /// and violating MS/TP timing (one bit at 76800 baud = 13 µs; a flash sector
    /// erase takes ~50 ms).
    ///
    #[allow(dead_code)]
    pub fn save(&mut self, config: &BridgeConfig) {
        // embassy-rp's flash.blocking_erase() calls multicore::pause_core1() internally,
        // so no manual Core 1 pause is needed here.

        let mut json_buf = [0u8; JSON_BUF_SIZE];
        let json_len = match serde_json_core::to_slice(config, &mut json_buf) {
            Ok(n) => n,
            Err(_) => {
                error!("config: JSON serialise error, not saving");
                return;
            }
        };

        // Pause Core 1 before flash operations (SIO_IRQ_FIFO ISR is in flash).
        let _pause = crate::core1::pause_core1_for_flash();

        // Erase the config region (CONFIG_SIZE = 32 KiB = 8 sectors).
        if let Err(_) = self
            .flash
            .blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + CONFIG_SIZE as u32)
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

// ---------------------------------------------------------------------------
// Async config save task
// ---------------------------------------------------------------------------

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

/// Signal to wake the config save task. Any task can call `request_save()`
/// and the dedicated save task will persist the config to flash.
static SAVE_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Request that the current in-memory config be saved to flash.
/// Non-blocking — the actual save happens asynchronously in `config_save_task`.
pub fn request_save() {
    SAVE_SIGNAL.signal(());
}

/// Scratch buffer for the config save task, held in a static to avoid putting
/// 4 KiB on the async task's Future stack (which would bloat every suspend point).
///
/// Safe: `config_save_task` runs as a single embassy task; no other code touches
/// this buffer.  Embassy tasks are `Send` + cooperative, so there is no
/// concurrent access.
static mut CONFIG_SAVE_BUF: [u8; JSON_BUF_SIZE] = [0u8; JSON_BUF_SIZE];

/// Dedicated task that waits for save requests and writes config to flash.
/// This keeps the JSON buffer off the web task stacks.
#[embassy_executor::task]
pub async fn config_save_task() {
    loop {
        SAVE_SIGNAL.wait().await;
        SAVE_SIGNAL.reset();

        // Brief delay to coalesce rapid saves (e.g. multiple config fields changed)
        embassy_time::Timer::after_millis(500).await;

        // SAFETY: this task is the sole writer; embassy cooperative scheduling
        // means the await points are the only places another task can run, and
        // no other task touches CONFIG_SAVE_BUF.
        #[allow(static_mut_refs)]
        let json_buf: &mut [u8; JSON_BUF_SIZE] = unsafe { &mut CONFIG_SAVE_BUF };

        let json_len = {
            let guard = crate::http::CONFIG.lock().await;
            match guard.as_ref() {
                Some(c) => match serde_json_core::to_slice(c, json_buf) {
                    Ok(n) => n,
                    Err(_) => {
                        error!("config: JSON serialise error, not saving");
                        continue;
                    }
                },
                None => continue,
            }
        };

        // Pause Core 1 and write flash
        let _pause = crate::core1::pause_core1_for_flash();

        let mut flash_guard = crate::ota::FLASH.lock().await;
        if let Some(flash) = flash_guard.as_mut() {
            if flash
                .blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + CONFIG_SIZE as u32)
                .is_err()
            {
                error!("config: flash erase error");
                continue;
            }
            let aligned_len = (json_len + 255) & !255;
            if flash
                .blocking_write(CONFIG_OFFSET, &json_buf[..aligned_len.min(JSON_BUF_SIZE)])
                .is_err()
            {
                error!("config: flash write error");
                continue;
            }
            info!("config: saved {} bytes to flash", json_len);
        }
    }
}
