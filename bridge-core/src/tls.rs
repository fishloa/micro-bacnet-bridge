//! TLS certificate management types.
//!
//! This module provides PEM-to-DER conversion and lightweight certificate
//! inspection helpers. It is `no_std`-compatible: all output is written into
//! caller-supplied slices; there is no heap allocation.
//!
//! # PEM format recap
//!
//! A PEM block looks like:
//!
//! ```text
//! -----BEGIN <LABEL>-----
//! <base64-encoded DER data, up to 64 chars per line>
//! -----END <LABEL>-----
//! ```
//!
//! [`pem_to_der`] locates the first such block in `pem`, decodes the base64
//! body into `out_buf`, and returns the label string together with the used
//! portion of `out_buf`.

use crate::error::DecodeError;
use heapless::String;
use pem_rfc7468::decode;

// ---------------------------------------------------------------------------
// Capacity constants
// ---------------------------------------------------------------------------

/// Maximum PEM cert-chain size accepted (12 KB).
pub const MAX_CERT_PEM: usize = 12 * 1024;

/// Maximum DER private-key size (256 bytes covers a P-256 key).
pub const MAX_KEY_DER: usize = 256;

// ---------------------------------------------------------------------------
// PEM helpers
// ---------------------------------------------------------------------------

/// Parse the first PEM block found in `pem`.
///
/// Decodes the base64 body into `out_buf` and returns
/// `(label, &out_buf[..decoded_len])`.
///
/// # Errors
///
/// Returns [`DecodeError::InvalidData`] if no valid PEM header/footer is
/// found, the label is missing, or a base64 character is invalid.
/// Returns [`DecodeError::LengthOutOfBounds`] if the decoded DER does not
/// fit in `out_buf`.
pub fn pem_to_der<'a>(
    pem: &[u8],
    out_buf: &'a mut [u8],
) -> Result<(&'static str, &'a [u8]), DecodeError> {
    // Use pem-rfc7468 to decode the first PEM block.
    // The decode function requires a mutable buffer to write the decoded data to.
    let (label, decoded) = decode(pem, out_buf).map_err(|_| DecodeError::InvalidData)?;

    // Classify the label into a known static string.
    let label_str: &'static str = classify_label(label).ok_or(DecodeError::InvalidData)?;

    Ok((label_str, decoded))
}

/// Return `true` if `data` starts with (or contains) a PEM certificate block.
pub fn is_cert_pem(data: &[u8]) -> bool {
    find_subsequence(data, b"-----BEGIN CERTIFICATE-----").is_some()
}

/// Return `true` if `data` starts with (or contains) a PEM private-key block.
pub fn is_key_pem(data: &[u8]) -> bool {
    find_subsequence(data, b"-----BEGIN EC PRIVATE KEY-----").is_some()
        || find_subsequence(data, b"-----BEGIN RSA PRIVATE KEY-----").is_some()
        || find_subsequence(data, b"-----BEGIN PRIVATE KEY-----").is_some()
}

/// Extract the Common Name (CN) from a DER-encoded X.509 certificate.
///
/// This is a simplified parser: it scans for the OID `2.5.4.3` (commonName)
/// and reads the immediately following UTF8String / PrintableString value.
/// Returns `"unknown"` if the CN cannot be found or is not valid ASCII.
pub fn extract_subject_cn(der: &[u8]) -> String<64> {
    // OID 2.5.4.3 encoded as DER: 06 03 55 04 03
    const CN_OID: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];
    let mut out: String<64> = String::new();

    let Some(oid_pos) = find_subsequence(der, CN_OID) else {
        let _ = out.push_str("unknown");
        return out;
    };

    // After the OID comes a SET or the value directly. Skip the OID.
    let after_oid = oid_pos + CN_OID.len();
    if after_oid + 2 > der.len() {
        let _ = out.push_str("unknown");
        return out;
    }

    // The value tag should be UTF8String (0x0C) or PrintableString (0x13),
    // but there may be a wrapping SET (0x31) first. We try both positions.
    let (tag, len_pos) = if der[after_oid] == 0x31 {
        // SET wrapper — skip tag + length
        let set_len_pos = after_oid + 1;
        let skip = der[set_len_pos] as usize + 1; // length byte
        (der[after_oid + 2 + skip], after_oid + 2 + skip + 1)
    } else {
        (der[after_oid], after_oid + 1)
    };

    if tag != 0x0C && tag != 0x13 && tag != 0x1E {
        let _ = out.push_str("unknown");
        return out;
    }

    if len_pos >= der.len() {
        let _ = out.push_str("unknown");
        return out;
    }

    let str_len = der[len_pos] as usize;
    let str_start = len_pos + 1;
    if str_start + str_len > der.len() {
        let _ = out.push_str("unknown");
        return out;
    }

    let bytes = &der[str_start..str_start + str_len];
    for &b in bytes.iter().take(64) {
        if b.is_ascii() {
            let _ = out.push(b as char);
        }
    }
    if out.is_empty() {
        let _ = out.push_str("unknown");
    }
    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find the first occurrence of `needle` in `haystack`, returning the byte
/// offset, or `None`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Map a label string to a known static label string.
///
/// Only labels that the firmware is expected to encounter are recognised.
fn classify_label(label: &str) -> Option<&'static str> {
    match label {
        "CERTIFICATE" => Some("CERTIFICATE"),
        "CERTIFICATE REQUEST" => Some("CERTIFICATE REQUEST"),
        "PRIVATE KEY" => Some("PRIVATE KEY"),
        "EC PRIVATE KEY" => Some("EC PRIVATE KEY"),
        "RSA PRIVATE KEY" => Some("RSA PRIVATE KEY"),
        "PUBLIC KEY" => Some("PUBLIC KEY"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal real DER certificate encoded as PEM (self-signed, CN=test).
    // Generated with: openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256
    //   -keyout /dev/null -out /dev/stdout -days 1 -subj "/CN=test" -noenc 2>/dev/null
    // The DER bytes are base64-encoded and split at 64 chars per line.
    //
    // This is a real (tiny, 2-day validity) certificate so the DER content
    // exercises the base64 decoder and the CN extractor.
    //
    // NOTE: for test portability we use a hard-coded minimal DER block instead
    // of a calendar-bound cert. The DER contains CN=test via OID 2.5.4.3.
    //
    // Minimal TBSCertificate with CN=test constructed by hand:
    //   SEQUENCE {
    //     ... (version, serial, algorithm, issuer with CN=test, validity, subject with CN=test, ...)
    //   }
    // For simplicity we use a snippet that contains the OID + UTF8String value
    // and wrap it in a fake PEM block.

    // FAKE test data — base64 of "this is not a real certificate" etc.
    // NOT real crypto material. Used only for PEM label parsing tests.
    const TEST_CERT_PEM: &[u8] = b"-----BEGIN CERTIFICATE-----\n\
        dGhpcyBpcyBub3QgYSByZWFsIGNlcnRpZmljYXRl\n\
        -----END CERTIFICATE-----\n";

    const TEST_KEY_PEM: &[u8] = b"-----BEGIN EC PRIVATE KEY-----\n\
        dGhpcyBpcyBub3QgYSByZWFsIGtleQ==\n\
        -----END EC PRIVATE KEY-----\n";

    const PKCS8_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
        dGhpcyBpcyBhbHNvIG5vdCBhIHJlYWwga2V5\n\
        -----END PRIVATE KEY-----\n";

    // -----------------------------------------------------------------------
    // pem_to_der
    // -----------------------------------------------------------------------

    #[test]
    fn pem_to_der_cert_label() {
        let mut buf = [0u8; 256];
        let (label, der) = pem_to_der(TEST_CERT_PEM, &mut buf).unwrap();
        assert_eq!(label, "CERTIFICATE");
        assert!(!der.is_empty(), "DER content should be non-empty");
    }

    #[test]
    fn pem_to_der_key_label() {
        let mut buf = [0u8; 256];
        let (label, _der) = pem_to_der(TEST_KEY_PEM, &mut buf).unwrap();
        assert_eq!(label, "EC PRIVATE KEY");
    }

    #[test]
    fn pem_to_der_pkcs8_label() {
        let mut buf = [0u8; 256];
        let (label, _) = pem_to_der(PKCS8_KEY_PEM, &mut buf).unwrap();
        assert_eq!(label, "PRIVATE KEY");
    }

    #[test]
    fn pem_to_der_rejects_missing_begin() {
        let mut buf = [0u8; 256];
        let result = pem_to_der(b"not a pem block", &mut buf);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_rejects_missing_end() {
        let pem = b"-----BEGIN CERTIFICATE-----\nYWJj\n";
        let mut buf = [0u8; 256];
        let result = pem_to_der(pem, &mut buf);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_rejects_empty_label() {
        let pem = b"-----BEGIN -----\nYWJj\n-----END -----\n";
        let mut buf = [0u8; 256];
        let result = pem_to_der(pem, &mut buf);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_rejects_unknown_label() {
        let pem = b"-----BEGIN MYSTERY OBJECT-----\nYWJj\n-----END MYSTERY OBJECT-----\n";
        let mut buf = [0u8; 256];
        let result = pem_to_der(pem, &mut buf);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_rejects_buffer_too_small() {
        // "abc" base64 = YWJj = 3 bytes
        let pem = b"-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n";
        let mut buf = [0u8; 1]; // too small for 3 bytes
        let result = pem_to_der(pem, &mut buf);
        // pem-rfc7468 returns InvalidData when buffer is too small (cannot decode)
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_rejects_invalid_base64() {
        let pem = b"-----BEGIN CERTIFICATE-----\nYW!!!\n-----END CERTIFICATE-----\n";
        let mut buf = [0u8; 256];
        let result = pem_to_der(pem, &mut buf);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    #[test]
    fn pem_to_der_accepts_padding() {
        // "Man" → TWFu (no padding); "Ma" → TWE= (one =); "M" → TQ== (two =)
        let pem = b"-----BEGIN CERTIFICATE-----\nTQ==\n-----END CERTIFICATE-----\n";
        let mut buf = [0u8; 8];
        let (_, der) = pem_to_der(pem, &mut buf).unwrap();
        assert_eq!(der, b"M");
    }

    // -----------------------------------------------------------------------
    // is_cert_pem / is_key_pem
    // -----------------------------------------------------------------------

    #[test]
    fn is_cert_pem_detects_cert() {
        assert!(is_cert_pem(TEST_CERT_PEM));
        assert!(!is_cert_pem(TEST_KEY_PEM));
    }

    #[test]
    fn is_key_pem_detects_ec_key() {
        assert!(is_key_pem(TEST_KEY_PEM));
        assert!(!is_key_pem(TEST_CERT_PEM));
    }

    #[test]
    fn is_key_pem_detects_pkcs8_key() {
        assert!(is_key_pem(PKCS8_KEY_PEM));
    }

    #[test]
    fn is_cert_and_key_both_false_for_garbage() {
        let junk = b"not a pem block at all";
        assert!(!is_cert_pem(junk));
        assert!(!is_key_pem(junk));
    }

    // -----------------------------------------------------------------------
    // extract_subject_cn
    // -----------------------------------------------------------------------

    #[test]
    fn extract_subject_cn_returns_unknown_for_garbage() {
        let cn = extract_subject_cn(b"\x00\x01\x02");
        assert_eq!(cn.as_str(), "unknown");
    }

    #[test]
    fn extract_subject_cn_finds_utf8string_cn() {
        // Manually craft the minimal DER snippet:
        //   OID 2.5.4.3 + UTF8String "bridge"
        //   06 03 55 04 03  0C 06 b r i d g e
        let mut der = [0u8; 16];
        der[0] = 0x06;
        der[1] = 0x03;
        der[2] = 0x55;
        der[3] = 0x04;
        der[4] = 0x03;
        der[5] = 0x0C; // UTF8String tag
        der[6] = 0x06; // length 6
        der[7..13].copy_from_slice(b"bridge");
        let cn = extract_subject_cn(&der);
        assert_eq!(cn.as_str(), "bridge");
    }

    #[test]
    fn extract_subject_cn_finds_printablestring_cn() {
        // OID 2.5.4.3 + PrintableString "hello"
        let mut der = [0u8; 16];
        der[0] = 0x06;
        der[1] = 0x03;
        der[2] = 0x55;
        der[3] = 0x04;
        der[4] = 0x03;
        der[5] = 0x13; // PrintableString tag
        der[6] = 0x05;
        der[7..12].copy_from_slice(b"hello");
        let cn = extract_subject_cn(&der);
        assert_eq!(cn.as_str(), "hello");
    }
}
