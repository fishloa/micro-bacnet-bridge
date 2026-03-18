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
// UF2 format support
// ---------------------------------------------------------------------------

/// UF2 block size (always 512 bytes).
pub const UF2_BLOCK_SIZE: usize = 512;

/// UF2 payload size per block (always 256 bytes).
pub const UF2_PAYLOAD_SIZE: usize = 256;

/// UF2 magic numbers.
pub const UF2_MAGIC1: u32 = 0x0A324655; // "UF2\n"
pub const UF2_MAGIC2: u32 = 0x9E5D5157;
pub const UF2_MAGIC3: u32 = 0x0AB16F30;

/// RP2040 family ID for UF2.
pub const UF2_FAMILY_RP2040: u32 = 0xE48BFF56;

/// Check if data starts with a valid UF2 magic header.
pub fn is_uf2(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == UF2_MAGIC1
}

/// Parse a single 512-byte UF2 block.
/// Returns `Ok((target_address, payload_slice, block_num, total_blocks))` or error.
pub fn parse_uf2_block(block: &[u8]) -> Result<(u32, &[u8], u32, u32), &'static str> {
    if block.len() < UF2_BLOCK_SIZE {
        return Err("block too short");
    }

    let magic1 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
    let magic2 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let _flags = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
    let target_addr = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    let payload_size = u32::from_le_bytes([block[16], block[17], block[18], block[19]]) as usize;
    let block_num = u32::from_le_bytes([block[20], block[21], block[22], block[23]]);
    let total_blocks = u32::from_le_bytes([block[24], block[25], block[26], block[27]]);
    let magic3 = u32::from_le_bytes([block[508], block[509], block[510], block[511]]);

    if magic1 != UF2_MAGIC1 {
        return Err("bad magic1");
    }
    if magic2 != UF2_MAGIC2 {
        return Err("bad magic2");
    }
    if magic3 != UF2_MAGIC3 {
        return Err("bad magic3");
    }
    if payload_size > UF2_PAYLOAD_SIZE {
        return Err("payload too large");
    }

    Ok((
        target_addr,
        &block[32..32 + payload_size],
        block_num,
        total_blocks,
    ))
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

    // --- UF2 tests ---

    fn make_uf2_block(target_addr: u32, block_num: u32, total: u32, payload: &[u8]) -> [u8; 512] {
        let mut block = [0u8; 512];
        block[0..4].copy_from_slice(&UF2_MAGIC1.to_le_bytes());
        block[4..8].copy_from_slice(&UF2_MAGIC2.to_le_bytes());
        block[8..12].copy_from_slice(&0u32.to_le_bytes()); // flags
        block[12..16].copy_from_slice(&target_addr.to_le_bytes());
        block[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        block[20..24].copy_from_slice(&block_num.to_le_bytes());
        block[24..28].copy_from_slice(&total.to_le_bytes());
        block[28..32].copy_from_slice(&UF2_FAMILY_RP2040.to_le_bytes());
        block[32..32 + payload.len()].copy_from_slice(payload);
        block[508..512].copy_from_slice(&UF2_MAGIC3.to_le_bytes());
        block
    }

    #[test]
    fn test_is_uf2_valid() {
        let block = make_uf2_block(0x10000100, 0, 1, &[0xAB; 256]);
        assert!(is_uf2(&block));
    }

    #[test]
    fn test_is_uf2_raw_binary() {
        // Raw ARM binary starts with SP (RAM address), not UF2 magic
        let hdr = make_header(0x2003_E000, 0x1000_0101);
        assert!(!is_uf2(&hdr));
    }

    #[test]
    fn test_is_uf2_too_short() {
        assert!(!is_uf2(&[0x55, 0x46]));
        assert!(!is_uf2(&[]));
    }

    #[test]
    fn test_parse_uf2_block_valid() {
        let payload = [0x42u8; 256];
        let block = make_uf2_block(0x10000100, 0, 10, &payload);
        let (addr, data, num, total) = parse_uf2_block(&block).unwrap();
        assert_eq!(addr, 0x10000100);
        assert_eq!(data.len(), 256);
        assert_eq!(data[0], 0x42);
        assert_eq!(num, 0);
        assert_eq!(total, 10);
    }

    #[test]
    fn test_parse_uf2_block_bad_magic() {
        let mut block = make_uf2_block(0x10000100, 0, 1, &[0; 256]);
        block[0] = 0xFF; // corrupt magic1
        assert!(parse_uf2_block(&block).is_err());
    }

    #[test]
    fn test_parse_uf2_block_too_short() {
        assert!(parse_uf2_block(&[0u8; 100]).is_err());
    }

    #[test]
    fn test_parse_uf2_small_payload() {
        let block = make_uf2_block(0x10000100, 0, 1, &[0xAA; 128]);
        let (_, data, _, _) = parse_uf2_block(&block).unwrap();
        assert_eq!(data.len(), 128);
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
