//! OTA firmware update support — platform-independent validation.
//!
//! This module contains the firmware image validation logic that runs on both
//! the device (no_std) and the host (for unit testing).

/// Maximum firmware image size accepted over HTTP (1.5 MB).
///
/// The flash budget for firmware is 1.5 MB; the remaining 512 KB at the top
/// of the 2 MB device flash is used for the config sector and headroom.
pub const MAX_FIRMWARE_SIZE: usize = 1_500_000;

/// Flash base address on RP2040 / RP2350 (XIP window start).
const FLASH_BASE: u32 = 0x1000_0000;

/// Maximum flash end address for a 4 MB device (RP2350 upper bound).
const FLASH_END_MAX: u32 = 0x1040_0000;

/// RAM base address (RP2040 and RP2350 share the same SRAM base).
const RAM_BASE: u32 = 0x2000_0000;

/// RAM end for RP2040 (264 KB SRAM).
const RAM_END_RP2040: u32 = 0x2004_2000;

/// RAM end for RP2350 (520 KB SRAM).
const RAM_END_RP2350: u32 = 0x2008_2000;

/// Validate the first 8 bytes of a firmware image.
///
/// An ARM Cortex-M firmware image begins with a vector table whose first two
/// 32-bit words are:
///
/// - Word 0 (offset 0x00): Initial stack pointer value — must point into SRAM.
/// - Word 1 (offset 0x04): Reset vector — must point into flash (with the LSB
///   set to 1 to indicate Thumb mode; we mask it off before range-checking).
///
/// # Returns
///
/// `true` if both words are within plausible ranges, `false` otherwise.
///
/// # Example
///
/// ```
/// use bridge_core::ota::validate_firmware_image;
///
/// // Stack pointer in RP2040 SRAM, reset vector in flash (Thumb bit set)
/// let mut hdr = [0u8; 8];
/// hdr[0..4].copy_from_slice(&0x2003_E000u32.to_le_bytes()); // SP
/// hdr[4..8].copy_from_slice(&0x1000_0101u32.to_le_bytes()); // reset vector | 1
/// assert!(validate_firmware_image(&hdr));
/// ```
pub fn validate_firmware_image(first_8_bytes: &[u8]) -> bool {
    if first_8_bytes.len() < 8 {
        return false;
    }

    let sp = u32::from_le_bytes([
        first_8_bytes[0],
        first_8_bytes[1],
        first_8_bytes[2],
        first_8_bytes[3],
    ]);
    let reset_raw = u32::from_le_bytes([
        first_8_bytes[4],
        first_8_bytes[5],
        first_8_bytes[6],
        first_8_bytes[7],
    ]);

    // Mask Thumb bit before range check.
    let reset_vec = reset_raw & !1;

    // SP must be word-aligned and in SRAM (accept either chip's SRAM size).
    let sp_valid =
        sp.is_multiple_of(4) && sp >= RAM_BASE && (sp <= RAM_END_RP2040 || sp <= RAM_END_RP2350);

    // Reset vector must be in flash.
    let rv_valid = (FLASH_BASE..FLASH_END_MAX).contains(&reset_vec);

    sp_valid && rv_valid
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(sp: u32, reset: u32) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&sp.to_le_bytes());
        buf[4..8].copy_from_slice(&reset.to_le_bytes());
        buf
    }

    // --- Positive cases ---

    #[test]
    fn test_validate_arm_vector_table_valid_rp2040() {
        // SP near top of RP2040 SRAM (264 KB = 0x42000 bytes above 0x20000000)
        let hdr = make_header(0x2004_1FFC, 0x1000_0101);
        assert!(
            validate_firmware_image(&hdr),
            "valid RP2040 SP + flash reset vector should pass"
        );
    }

    #[test]
    fn test_validate_arm_vector_table_valid_rp2350() {
        // SP near top of RP2350 SRAM (520 KB)
        let hdr = make_header(0x2008_1FF0, 0x1000_0201);
        assert!(
            validate_firmware_image(&hdr),
            "valid RP2350 SP + flash reset vector should pass"
        );
    }

    #[test]
    fn test_validate_thumb_bit_stripped_from_reset_vector() {
        // Reset vector with Thumb bit set (bit 0 = 1) — must still pass.
        let hdr = make_header(0x2003_E000, 0x1000_0101);
        assert!(
            validate_firmware_image(&hdr),
            "Thumb bit in reset vector should be stripped before range check"
        );
    }

    #[test]
    fn test_validate_reset_vector_at_flash_base() {
        // Lowest valid flash address (boot2 loader).
        let hdr = make_header(0x2002_0000, FLASH_BASE | 1);
        assert!(
            validate_firmware_image(&hdr),
            "reset vector at flash base should pass"
        );
    }

    #[test]
    fn test_validate_sp_at_ram_base_plus_four() {
        // Minimal valid SP: just above RAM base and word-aligned.
        let hdr = make_header(RAM_BASE + 4, 0x1000_0101);
        assert!(
            validate_firmware_image(&hdr),
            "SP just above RAM base should pass"
        );
    }

    // --- Negative cases ---

    #[test]
    fn test_reject_invalid_vector_table_sp_zero() {
        let hdr = make_header(0x0000_0000, 0x1000_0101);
        assert!(!validate_firmware_image(&hdr), "SP == 0 should fail");
    }

    #[test]
    fn test_reject_invalid_vector_table_sp_in_flash() {
        // SP pointing into flash, not RAM.
        let hdr = make_header(0x1000_8000, 0x1000_0101);
        assert!(
            !validate_firmware_image(&hdr),
            "SP in flash range should fail"
        );
    }

    #[test]
    fn test_reject_invalid_vector_table_sp_beyond_ram() {
        // SP above both RP2040 and RP2350 SRAM ceilings.
        let hdr = make_header(0x2009_0000, 0x1000_0101);
        assert!(
            !validate_firmware_image(&hdr),
            "SP beyond max SRAM should fail"
        );
    }

    #[test]
    fn test_reject_invalid_vector_table_reset_vec_in_ram() {
        // Reset vector pointing into RAM is nonsensical.
        let hdr = make_header(0x2003_E000, 0x2000_0101);
        assert!(
            !validate_firmware_image(&hdr),
            "reset vector in RAM should fail"
        );
    }

    #[test]
    fn test_reject_invalid_vector_table_reset_vec_zero() {
        let hdr = make_header(0x2003_E000, 0x0000_0000);
        assert!(
            !validate_firmware_image(&hdr),
            "reset vector == 0 should fail"
        );
    }

    #[test]
    fn test_reject_too_short_buffer() {
        // Buffer with fewer than 8 bytes must be rejected.
        assert!(
            !validate_firmware_image(&[0u8; 7]),
            "buffer shorter than 8 bytes should fail"
        );
        assert!(!validate_firmware_image(&[]), "empty buffer should fail");
    }

    #[test]
    fn test_reject_misaligned_sp() {
        // SP not word-aligned (not divisible by 4).
        let hdr = make_header(0x2003_E001, 0x1000_0101);
        assert!(!validate_firmware_image(&hdr), "misaligned SP should fail");
    }

    #[test]
    fn test_max_firmware_size_constant() {
        assert_eq!(MAX_FIRMWARE_SIZE, 1_500_000);
    }

    // --- Regression: clippy is_multiple_of / range contains refactor ---

    /// Regression: SP at exact RAM_BASE must pass (word-aligned, in range).
    #[test]
    fn test_sp_at_exact_ram_base() {
        let hdr = make_header(RAM_BASE, 0x1000_0101);
        assert!(
            validate_firmware_image(&hdr),
            "SP == RAM_BASE should pass (word-aligned, in SRAM)"
        );
    }

    /// Regression: reset vector at exact FLASH_BASE (no Thumb bit) must pass.
    #[test]
    fn test_reset_vec_at_exact_flash_base_no_thumb() {
        let hdr = make_header(0x2003_E000, FLASH_BASE);
        assert!(
            validate_firmware_image(&hdr),
            "reset vector at FLASH_BASE (even, no Thumb bit) should pass"
        );
    }

    /// Regression: reset vector at FLASH_END_MAX must fail (exclusive upper bound).
    #[test]
    fn test_reset_vec_at_flash_end_max_fails() {
        let hdr = make_header(0x2003_E000, FLASH_END_MAX | 1);
        assert!(
            !validate_firmware_image(&hdr),
            "reset vector at FLASH_END_MAX should fail (exclusive bound)"
        );
    }

    /// Regression: SP == 2 (word-aligned but below RAM_BASE) must fail.
    #[test]
    fn test_sp_below_ram_base_fails() {
        let hdr = make_header(4, 0x1000_0101);
        assert!(
            !validate_firmware_image(&hdr),
            "SP below RAM_BASE should fail"
        );
    }
}
