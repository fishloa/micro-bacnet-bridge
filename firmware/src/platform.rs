//! Platform-specific constants for W5500-EVB-Pico2 (RP2350A).
//!
//! When porting to a different board, only this file and memory.x need to change.

/// Total flash size in bytes.
pub const FLASH_SIZE: usize = 4 * 1024 * 1024;

/// Flash erase sector size.
pub const SECTOR_SIZE: usize = 4096;

/// Flash page size (write alignment).
#[allow(dead_code)]
pub const PAGE_SIZE: usize = 256;

/// Config region offset from flash start (top of flash - 64 KiB).
pub const CONFIG_OFFSET: u32 = (FLASH_SIZE - 64 * 1024) as u32;

/// Config region size.
pub const CONFIG_SIZE: usize = 32 * 1024;

/// Identity sector offset (MAC address, top of flash - 4 KiB).
pub const IDENTITY_OFFSET: u32 = (FLASH_SIZE - 4 * 1024) as u32;

/// OTA firmware write start offset. The full binary including vector table
/// and boot block is uploaded and written from the start of flash.
pub const FIRMWARE_OFFSET: u32 = 0;

/// Flash offset beyond which OTA must not write (protects config + identity).
pub const PROTECTED_OFFSET: u32 = CONFIG_OFFSET;
