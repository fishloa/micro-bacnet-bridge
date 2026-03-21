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

/// Slot A: firmware at flash offset 0 (default boot location).
pub const SLOT_A_OFFSET: u32 = 0;

/// Slot B: alternate firmware location for A/B OTA.
pub const SLOT_B_OFFSET: u32 = (FLASH_SIZE / 2) as u32; // 1 MB

/// Flash offset beyond which OTA must not write (protects config + identity).
pub const PROTECTED_OFFSET: u32 = CONFIG_OFFSET;

/// XIP base address for address comparisons.
pub const XIP_BASE: u32 = 0x10000000;

/// Layout (2MB flash):
///   [Slot A:   0x000000-0x0FFFFF] (1MB)
///   [Slot B:   0x100000-0x1EFFFF] (960KB)
///   [Config:   0x1F0000-0x1FBFFF] (48KB)
///   [Identity: 0x1FF000-0x1FFFFF] (4KB)
///
/// OTA writes to the INACTIVE slot, then reboots into it.
/// After probe flash, Slot A is active.
/// After first OTA, Slot B is active.
/// After second OTA, Slot A is active (via normal reboot).

/// Determine which slot we're currently running from.
/// Returns the offset of the INACTIVE slot (where OTA should write).
#[inline]
pub fn ota_target_slot() -> u32 {
    // Check if our code is running from Slot A or Slot B.
    // Function address will be in 0x100xxxxx (Slot A) or 0x101xxxxx (Slot B).
    let pc: u32;
    unsafe { core::arch::asm!("mov {}, pc", out(reg) pc) };
    if pc < XIP_BASE + SLOT_B_OFFSET {
        // Running from Slot A → write to Slot B
        SLOT_B_OFFSET
    } else {
        // Running from Slot B → write to Slot A
        SLOT_A_OFFSET
    }
}
