//! Platform-specific constants for W5500-EVB-Pico2 (RP2350A).
//!
//! When porting to a different board, only this file and memory.x need to change.

/// Total flash size in bytes.
/// W5500-EVB-Pico2 has 2MB (16Mbit) flash, NOT 4MB like the Raspberry Pi Pico 2.
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;

/// Flash erase sector size.
pub const SECTOR_SIZE: usize = 4096;

/// Flash page size (write alignment).
#[allow(dead_code)]
pub const PAGE_SIZE: usize = 256;

/// Config region offset from flash start (top of flash - 64 KiB).
/// On 2MB flash: 0x1F0000.
pub const CONFIG_OFFSET: u32 = (FLASH_SIZE - 64 * 1024) as u32;

/// Config region size.
pub const CONFIG_SIZE: usize = 32 * 1024;

/// Identity sector offset (MAC address, top of flash - 4 KiB).
/// On 2MB flash: 0x1FF000.
pub const IDENTITY_OFFSET: u32 = (FLASH_SIZE - 4 * 1024) as u32;

/// OTA firmware write start offset. The full binary including vector table
/// and boot block is uploaded and written from the start of flash.
pub const FIRMWARE_OFFSET: u32 = 0;

/// Flash offset beyond which OTA must not write (protects config + identity).
pub const PROTECTED_OFFSET: u32 = CONFIG_OFFSET;

/// OTA staging area — upper half of flash, before the config region.
/// Layout (2MB flash):
///   [Firmware: 0x000000-0x0FFFFF] (1MB max)
///   [Staging:  0x100000-0x1EFFFF] (960KB)
///   [Config:   0x1F0000-0x1FBFFF] (48KB)
///   [Identity: 0x1FF000-0x1FFFFF] (4KB)
pub const STAGING_OFFSET: u32 = (FLASH_SIZE / 2) as u32; // 1 MB
