//! OTA firmware update support — platform-independent validation and manifest parsing.
//!
//! This module contains the firmware image validation logic that runs on both
//! the device (no_std) and the host (for unit testing), plus manifest parsing
//! for the auto-update channel system.

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
// Manifest parsing
// ---------------------------------------------------------------------------

use crate::error::DecodeError;
use heapless::String;

/// A parsed firmware manifest entry.
///
/// The manifest JSON format served by the update server is:
/// ```json
/// {
///   "channels": {
///     "release": {
///       "version": "0.1.42",
///       "url": "https://example.com/firmware.uf2",
///       "sha256": "aabbcc...",
///       "size": 524288
///     },
///     "beta": { ... }
///   }
/// }
/// ```
#[derive(Debug, PartialEq)]
pub struct ManifestEntry {
    /// Firmware version string (e.g. `"0.1.42-pico2"`).
    pub version: String<32>,
    /// Download URL for the firmware binary.
    pub url: String<128>,
    /// SHA-256 checksum of the firmware file (32 raw bytes).
    pub sha256: [u8; 32],
    /// Expected file size in bytes.
    pub size: u32,
}

/// Parse a manifest JSON response for a given channel.
///
/// Locates the channel key inside `{"channels":{...}}` and extracts the
/// `version`, `url`, `sha256`, and `size` fields using a minimal JSON scanner
/// (no allocator required).
///
/// # Errors
///
/// Returns [`DecodeError::InvalidData`] if:
/// - The JSON structure is missing the `channels` or channel key.
/// - Any required field is absent or malformed.
///
/// Returns [`DecodeError::LengthOutOfBounds`] if a string value exceeds the
/// corresponding capacity constant.
pub fn parse_manifest(json: &[u8], channel: &str) -> Result<ManifestEntry, DecodeError> {
    // Locate "channels":
    let channels_key = b"\"channels\"";
    let ch_pos = find_bytes(json, channels_key).ok_or(DecodeError::InvalidData)?;

    // Find the opening '{' of the channels object.
    let brace_pos =
        find_after(json, ch_pos + channels_key.len(), b'{').ok_or(DecodeError::InvalidData)?;

    // Build the quoted channel name key.
    let mut chan_key = [0u8; 34];
    let chan_key_len = build_quoted_key(channel, &mut chan_key)?;
    let chan_key = &chan_key[..chan_key_len];

    // Find the channel key inside the channels object.
    let chan_pos = find_bytes(&json[brace_pos..], chan_key).ok_or(DecodeError::InvalidData)?;
    let chan_start = brace_pos + chan_pos + chan_key.len();

    // Find the opening '{' of this channel's object.
    let chan_brace = find_after(json, chan_start, b'{').ok_or(DecodeError::InvalidData)?;

    // Find the matching closing '}' — we'll search for fields within this range.
    let chan_end = find_closing_brace(json, chan_brace).ok_or(DecodeError::InvalidData)?;
    let chan_obj = &json[chan_brace..=chan_end];

    // Extract fields.
    let version_raw = extract_string_field(chan_obj, b"version")?;
    let url_raw = extract_string_field(chan_obj, b"url")?;
    let sha256_raw = extract_string_field(chan_obj, b"sha256")?;
    let size_raw = extract_number_field(chan_obj, b"size")?;

    // Build ManifestEntry.
    let mut version: String<32> = String::new();
    push_bytes(&mut version, version_raw)?;

    let mut url: String<128> = String::new();
    push_bytes(&mut url, url_raw)?;

    let sha256 = parse_sha256_hex(sha256_raw)?;

    Ok(ManifestEntry {
        version,
        url,
        sha256,
        size: size_raw,
    })
}

/// Compare two version strings (semver-like: `"major.minor.patch[-suffix]"`).
///
/// Returns `true` if `available` is strictly newer than `current` by numeric
/// component comparison. Suffixes (text after `-`) are ignored for ordering.
///
/// # Examples
///
/// ```
/// use bridge_core::ota::is_newer_version;
/// assert!(is_newer_version("0.1.41", "0.1.42"));
/// assert!(is_newer_version("0.1.99", "0.2.0"));
/// assert!(!is_newer_version("0.1.42", "0.1.42"));
/// assert!(!is_newer_version("1.0.0", "0.9.9"));
/// ```
pub fn is_newer_version(current: &str, available: &str) -> bool {
    let cur = parse_semver(current);
    let avail = parse_semver(available);
    avail > cur
}

// ---------------------------------------------------------------------------
// Manifest parsing internals
// ---------------------------------------------------------------------------

/// Scan for the first occurrence of `needle` in `haystack`.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Scan for the first occurrence of byte `b` in `data` at or after `start`.
fn find_after(data: &[u8], start: usize, b: u8) -> Option<usize> {
    data[start..]
        .iter()
        .position(|&x| x == b)
        .map(|p| start + p)
}

/// Build `"<key>"` into `buf` (including quotes). Returns bytes written.
fn build_quoted_key(key: &str, buf: &mut [u8; 34]) -> Result<usize, DecodeError> {
    let kbytes = key.as_bytes();
    if kbytes.len() + 2 > buf.len() {
        return Err(DecodeError::LengthOutOfBounds);
    }
    buf[0] = b'"';
    buf[1..1 + kbytes.len()].copy_from_slice(kbytes);
    buf[1 + kbytes.len()] = b'"';
    Ok(2 + kbytes.len())
}

/// Find the matching `}` for the `{` at `open_pos` in `data`.
fn find_closing_brace(data: &[u8], open_pos: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in data[open_pos..].iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open_pos + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract the string value of a JSON field `"key": "value"` from `obj`.
/// Returns the raw bytes of the value (between the quotes).
fn extract_string_field<'a>(obj: &'a [u8], key: &[u8]) -> Result<&'a [u8], DecodeError> {
    // Build `"key"` to search for.
    let mut needle = [0u8; 66];
    if key.len() + 2 > needle.len() {
        return Err(DecodeError::InvalidData);
    }
    needle[0] = b'"';
    needle[1..1 + key.len()].copy_from_slice(key);
    needle[1 + key.len()] = b'"';
    let needle = &needle[..2 + key.len()];

    let key_pos = find_bytes(obj, needle).ok_or(DecodeError::InvalidData)?;
    let after_key = key_pos + needle.len();

    // Skip whitespace and ':'
    let colon = find_after(obj, after_key, b':').ok_or(DecodeError::InvalidData)?;
    let value_quote = find_after(obj, colon + 1, b'"').ok_or(DecodeError::InvalidData)?;
    let value_start = value_quote + 1;

    // Find closing quote (not preceded by backslash — simplified).
    let closing = obj[value_start..]
        .iter()
        .position(|&b| b == b'"')
        .ok_or(DecodeError::InvalidData)?;

    Ok(&obj[value_start..value_start + closing])
}

/// Extract the numeric value of a JSON field `"key": <number>` from `obj`.
fn extract_number_field(obj: &[u8], key: &[u8]) -> Result<u32, DecodeError> {
    let mut needle = [0u8; 66];
    if key.len() + 2 > needle.len() {
        return Err(DecodeError::InvalidData);
    }
    needle[0] = b'"';
    needle[1..1 + key.len()].copy_from_slice(key);
    needle[1 + key.len()] = b'"';
    let needle = &needle[..2 + key.len()];

    let key_pos = find_bytes(obj, needle).ok_or(DecodeError::InvalidData)?;
    let after_key = key_pos + needle.len();
    let colon = find_after(obj, after_key, b':').ok_or(DecodeError::InvalidData)?;

    // Skip whitespace.
    let mut digit_start = colon + 1;
    while digit_start < obj.len()
        && (obj[digit_start] == b' ' || obj[digit_start] == b'\t' || obj[digit_start] == b'\n')
    {
        digit_start += 1;
    }

    let mut value = 0u32;
    let mut found_digit = false;
    for &b in &obj[digit_start..] {
        if b.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add((b - b'0') as u32);
            found_digit = true;
        } else {
            break;
        }
    }
    if !found_digit {
        return Err(DecodeError::InvalidData);
    }
    Ok(value)
}

/// Parse a 64-char hex string into 32 raw bytes.
fn parse_sha256_hex(hex: &[u8]) -> Result<[u8; 32], DecodeError> {
    if hex.len() != 64 {
        return Err(DecodeError::InvalidData);
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(hex[2 * i])?;
        let lo = hex_nibble(hex[2 * i + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, DecodeError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(DecodeError::InvalidData),
    }
}

/// Push ASCII bytes into a `heapless::String`.
fn push_bytes<const N: usize>(s: &mut String<N>, bytes: &[u8]) -> Result<(), DecodeError> {
    for &b in bytes {
        s.push(b as char)
            .map_err(|_| DecodeError::LengthOutOfBounds)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Semver comparison
// ---------------------------------------------------------------------------

/// Parse a semver-like version string into `(major, minor, patch)`.
/// Suffix after `-` is discarded.
fn parse_semver(s: &str) -> (u32, u32, u32) {
    let base = s.split('-').next().unwrap_or(s);
    let mut parts = base.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor, patch)
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

    // -----------------------------------------------------------------------
    // parse_manifest tests
    // -----------------------------------------------------------------------

    const VALID_SHA256: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

    fn valid_manifest_json(channel: &str) -> heapless::String<512> {
        use core::fmt::Write;
        let mut s: heapless::String<512> = heapless::String::new();
        write!(
            s,
            r#"{{"channels":{{"{}":{{"version":"0.1.42","url":"https://example.com/fw.uf2","sha256":"{}","size":524288}}}}}}"#,
            channel, VALID_SHA256
        ).ok();
        s
    }

    #[test]
    fn parse_manifest_valid_release() {
        let json = valid_manifest_json("release");
        let entry = parse_manifest(json.as_bytes(), "release").unwrap();
        assert_eq!(entry.version.as_str(), "0.1.42");
        assert_eq!(entry.url.as_str(), "https://example.com/fw.uf2");
        assert_eq!(entry.size, 524288);
        // SHA256: first two bytes should be 0xAA 0xBB
        assert_eq!(entry.sha256[0], 0xAA);
        assert_eq!(entry.sha256[1], 0xBB);
    }

    #[test]
    fn parse_manifest_valid_beta_channel() {
        let mut json: heapless::String<512> = heapless::String::new();
        use core::fmt::Write;
        write!(
            json,
            r#"{{"channels":{{"release":{{"version":"0.1.40","url":"https://example.com/r.uf2","sha256":"{}","size":100000}},"beta":{{"version":"0.2.0","url":"https://example.com/b.uf2","sha256":"{}","size":200000}}}}}}"#,
            VALID_SHA256, VALID_SHA256
        ).ok();
        let entry = parse_manifest(json.as_bytes(), "beta").unwrap();
        assert_eq!(entry.version.as_str(), "0.2.0");
        assert_eq!(entry.size, 200000);
    }

    #[test]
    fn parse_manifest_missing_channel_returns_error() {
        let json = valid_manifest_json("release");
        let result = parse_manifest(json.as_bytes(), "nightly");
        assert!(result.is_err(), "missing channel should return error");
    }

    #[test]
    fn parse_manifest_missing_channels_key_returns_error() {
        let json = b"{\"version\":\"0.1.0\"}";
        let result = parse_manifest(json, "release");
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn parse_manifest_bad_sha256_length_returns_error() {
        let json = br#"{"channels":{"release":{"version":"0.1.0","url":"https://x.com/f.uf2","sha256":"tooshort","size":100}}}"#;
        let result = parse_manifest(json, "release");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // is_newer_version tests
    // -----------------------------------------------------------------------

    #[test]
    fn newer_patch_version() {
        assert!(is_newer_version("0.1.41", "0.1.42"));
    }

    #[test]
    fn newer_minor_version() {
        assert!(is_newer_version("0.1.99", "0.2.0"));
    }

    #[test]
    fn newer_major_version() {
        assert!(is_newer_version("0.9.99", "1.0.0"));
    }

    #[test]
    fn equal_version_returns_false() {
        assert!(!is_newer_version("0.1.42", "0.1.42"));
    }

    #[test]
    fn older_version_returns_false() {
        assert!(!is_newer_version("1.0.0", "0.9.9"));
    }

    #[test]
    fn suffix_ignored_in_comparison() {
        // "0.1.42-pico2" has the same base as "0.1.42" → not newer
        assert!(!is_newer_version("0.1.42-pico2", "0.1.42"));
        // "0.1.43-pico2" is newer than "0.1.42"
        assert!(is_newer_version("0.1.42", "0.1.43-pico2"));
    }
}
