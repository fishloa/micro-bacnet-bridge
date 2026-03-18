//! SNTP packet codec (RFC 4330).
//!
//! Implements a minimal SNTP v4 client codec:
//! - [`encode_request`] — builds a 48-byte SNTP request packet (client mode).
//! - [`decode_response`] — parses an SNTP response and extracts the transmit
//!   timestamp.
//! - [`ntp_to_unix_epoch`] — converts NTP seconds (epoch 1900-01-01) to Unix
//!   seconds (epoch 1970-01-01).
//!
//! This module is `no_std` and allocates nothing.

use crate::error::DecodeError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Length of an SNTP packet on the wire (RFC 4330 §4).
pub const SNTP_PACKET_LEN: usize = 48;

/// NTP UDP port.
pub const NTP_PORT: u16 = 123;

/// Number of seconds between the NTP epoch (1900-01-01) and the Unix epoch
/// (1970-01-01): 70 years including 17 leap years.
///
/// 70 * 365 * 86400 + 17 * 86400 = 2_208_988_800
pub const NTP_UNIX_OFFSET: u32 = 2_208_988_800;

// ---------------------------------------------------------------------------
// NtpTimestamp
// ---------------------------------------------------------------------------

/// An NTP timestamp (RFC 4330 §3).
///
/// `seconds` is seconds since **1900-01-01 00:00:00 UTC**.
/// `fraction` is the fractional part in units of 2^-32 seconds (sub-second
/// resolution of ~0.23 ns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpTimestamp {
    /// Whole seconds since 1900-01-01.
    pub seconds: u32,
    /// Fractional seconds (2^-32 s units).
    pub fraction: u32,
}

// ---------------------------------------------------------------------------
// NtpPacket
// ---------------------------------------------------------------------------

/// A parsed SNTP packet (48 bytes, RFC 4330 §4).
///
/// All multi-byte fields are stored in host byte order after decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtpPacket {
    /// Leap indicator (bits 7-6), version (bits 5-3), mode (bits 2-0).
    pub li_vn_mode: u8,
    /// Stratum level of the local clock.
    pub stratum: u8,
    /// Maximum interval between successive messages (log2 seconds).
    pub poll: u8,
    /// Precision of the local clock (log2 seconds, signed).
    pub precision: i8,
    /// Round-trip delay to the primary reference source (32-bit fixed point).
    pub root_delay: u32,
    /// Nominal error relative to the primary reference source.
    pub root_dispersion: u32,
    /// Reference ID (4 ASCII chars for stratum 1, IP for stratum ≥ 2).
    pub ref_id: u32,
    /// Reference timestamp (when the system clock was last set or corrected).
    pub ref_ts: NtpTimestamp,
    /// Originate timestamp (when the request departed the client).
    pub origin_ts: NtpTimestamp,
    /// Receive timestamp (when the request arrived at the server).
    pub recv_ts: NtpTimestamp,
    /// Transmit timestamp (when the response departed the server).
    pub transmit_ts: NtpTimestamp,
}

// ---------------------------------------------------------------------------
// Encode
// ---------------------------------------------------------------------------

/// Encode a 48-byte SNTP client request into `buf`.
///
/// Sets version = 4, mode = 3 (client), all timestamps = 0. This is the
/// minimal SNTP v4 request as specified in RFC 4330 §5.
///
/// # Panics
/// Panics in debug builds if `buf.len() < 48`. In release builds the function
/// writes 0 bytes and returns 0 when the buffer is too small.
///
/// # Returns
/// The number of bytes written (always 48 on success, 0 if buffer too small).
pub fn encode_request(buf: &mut [u8]) -> usize {
    if buf.len() < SNTP_PACKET_LEN {
        return 0;
    }
    // Zero the entire 48-byte packet
    for b in buf[..SNTP_PACKET_LEN].iter_mut() {
        *b = 0;
    }
    // LI=0 (no warning), VN=4 (version 4), Mode=3 (client)
    // Packed into byte 0: (LI << 6) | (VN << 3) | Mode
    //   = (0 << 6) | (4 << 3) | 3 = 0b00_100_011 = 0x23
    buf[0] = 0x23;
    SNTP_PACKET_LEN
}

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

/// Decode an SNTP server response from `data` and return the transmit
/// timestamp.
///
/// The transmit timestamp (bytes 40–47) is the time at which the server sent
/// the response and is used as the best estimate of current UTC time.
///
/// # Errors
/// Returns [`DecodeError::UnexpectedEnd`] if `data.len() < 48`.
/// Returns [`DecodeError::InvalidVersion`] if the NTP version field is not 3
/// or 4.
/// Returns [`DecodeError::InvalidData`] if the server stratum is 0 (the
/// "kiss-of-death" stratum, indicating the server refused the request).
pub fn decode_response(data: &[u8]) -> Result<NtpTimestamp, DecodeError> {
    if data.len() < SNTP_PACKET_LEN {
        return Err(DecodeError::UnexpectedEnd);
    }

    // Byte 0: LI (bits 7-6) | VN (bits 5-3) | Mode (bits 2-0)
    let version = (data[0] >> 3) & 0x07;
    if version != 3 && version != 4 {
        return Err(DecodeError::InvalidVersion);
    }

    // Stratum 0 means "kiss-of-death" — the server is telling us to go away.
    let stratum = data[1];
    if stratum == 0 {
        return Err(DecodeError::InvalidData);
    }

    // Transmit timestamp is at bytes 40..47 (two u32 big-endian: seconds, fraction)
    let ts = read_ntp_timestamp(data, 40);
    Ok(ts)
}

/// Decode a full SNTP packet from `data`.
///
/// Returns `Err(DecodeError::UnexpectedEnd)` if `data` is shorter than 48 bytes.
pub fn decode_packet(data: &[u8]) -> Result<NtpPacket, DecodeError> {
    if data.len() < SNTP_PACKET_LEN {
        return Err(DecodeError::UnexpectedEnd);
    }
    Ok(NtpPacket {
        li_vn_mode: data[0],
        stratum: data[1],
        poll: data[2],
        precision: data[3] as i8,
        root_delay: read_u32(data, 4),
        root_dispersion: read_u32(data, 8),
        ref_id: read_u32(data, 12),
        ref_ts: read_ntp_timestamp(data, 16),
        origin_ts: read_ntp_timestamp(data, 24),
        recv_ts: read_ntp_timestamp(data, 32),
        transmit_ts: read_ntp_timestamp(data, 40),
    })
}

// ---------------------------------------------------------------------------
// Epoch conversion
// ---------------------------------------------------------------------------

/// Convert an NTP timestamp (seconds since 1900-01-01) to a Unix timestamp
/// (seconds since 1970-01-01).
///
/// Returns `None` if `ntp_secs` is less than [`NTP_UNIX_OFFSET`], which would
/// represent a date before the Unix epoch and indicates a malformed server
/// response.
pub fn ntp_to_unix_epoch(ntp_secs: u32) -> Option<u32> {
    ntp_secs.checked_sub(NTP_UNIX_OFFSET)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[inline]
fn read_u32(data: &[u8], offset: usize) -> u32 {
    ((data[offset] as u32) << 24)
        | ((data[offset + 1] as u32) << 16)
        | ((data[offset + 2] as u32) << 8)
        | (data[offset + 3] as u32)
}

#[inline]
fn read_ntp_timestamp(data: &[u8], offset: usize) -> NtpTimestamp {
    NtpTimestamp {
        seconds: read_u32(data, offset),
        fraction: read_u32(data, offset + 4),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- encode_request ----------------------------------------------------

    #[test]
    fn encode_request_length() {
        let mut buf = [0u8; 48];
        let n = encode_request(&mut buf);
        assert_eq!(n, 48);
    }

    #[test]
    fn encode_request_li_vn_mode() {
        let mut buf = [0u8; 48];
        encode_request(&mut buf);
        // LI=0, VN=4, Mode=3 → 0b00_100_011 = 0x23
        assert_eq!(buf[0], 0x23);
    }

    #[test]
    fn encode_request_version_field() {
        let mut buf = [0u8; 48];
        encode_request(&mut buf);
        let version = (buf[0] >> 3) & 0x07;
        assert_eq!(version, 4, "version must be 4");
    }

    #[test]
    fn encode_request_mode_field() {
        let mut buf = [0u8; 48];
        encode_request(&mut buf);
        let mode = buf[0] & 0x07;
        assert_eq!(mode, 3, "mode must be 3 (client)");
    }

    #[test]
    fn encode_request_all_timestamps_zero() {
        let mut buf = [0xFFu8; 48];
        encode_request(&mut buf);
        // All bytes except byte 0 should be zero
        assert!(
            buf[1..].iter().all(|&b| b == 0),
            "all fields except byte 0 must be zero"
        );
    }

    #[test]
    fn encode_request_buffer_too_small_returns_zero() {
        let mut buf = [0u8; 47];
        let n = encode_request(&mut buf);
        assert_eq!(n, 0, "buffer too small must return 0");
    }

    #[test]
    fn encode_request_exact_size_buffer() {
        let mut buf = [0u8; 48];
        let n = encode_request(&mut buf);
        assert_eq!(n, 48);
    }

    #[test]
    fn encode_request_larger_buffer() {
        let mut buf = [0xFFu8; 64];
        let n = encode_request(&mut buf);
        assert_eq!(n, 48);
        // Bytes beyond 48 should be untouched
        assert!(buf[48..].iter().all(|&b| b == 0xFF));
    }

    // ---- decode_response ---------------------------------------------------

    /// Build a minimal valid SNTP response packet.
    fn make_valid_response(transmit_secs: u32, transmit_frac: u32) -> [u8; 48] {
        let mut pkt = [0u8; 48];
        // LI=0, VN=4, Mode=4 (server)
        pkt[0] = (4 << 3) | 4; // 0x24
        pkt[1] = 1; // stratum 1 (primary reference)
        pkt[2] = 6; // poll interval
        pkt[3] = 0xEC_u8; // precision = -20 (i8)
                          // Transmit timestamp at bytes 40..47
        pkt[40] = (transmit_secs >> 24) as u8;
        pkt[41] = (transmit_secs >> 16) as u8;
        pkt[42] = (transmit_secs >> 8) as u8;
        pkt[43] = transmit_secs as u8;
        pkt[44] = (transmit_frac >> 24) as u8;
        pkt[45] = (transmit_frac >> 16) as u8;
        pkt[46] = (transmit_frac >> 8) as u8;
        pkt[47] = transmit_frac as u8;
        pkt
    }

    #[test]
    fn decode_response_valid() {
        // 2024-01-01 00:00:00 UTC = Unix 1704067200 + NTP offset = 3913055200 NTP
        let ntp_secs: u32 = 3_913_055_200;
        let pkt = make_valid_response(ntp_secs, 0x80000000); // 0.5 seconds fraction
        let ts = decode_response(&pkt).unwrap();
        assert_eq!(ts.seconds, ntp_secs);
        assert_eq!(ts.fraction, 0x80000000);
    }

    #[test]
    fn decode_response_too_short() {
        let pkt = [0u8; 47];
        assert_eq!(
            decode_response(&pkt).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
    }

    #[test]
    fn decode_response_empty() {
        assert_eq!(
            decode_response(&[]).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
    }

    #[test]
    fn decode_response_bad_version_zero() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[0] = 0x00; // LI=0, VN=0, Mode=0 — version 0 is invalid
        assert_eq!(
            decode_response(&pkt).unwrap_err(),
            DecodeError::InvalidVersion
        );
    }

    #[test]
    fn decode_response_bad_version_1() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[0] = (1 << 3) | 4; // VN=1 — not 3 or 4
        assert_eq!(
            decode_response(&pkt).unwrap_err(),
            DecodeError::InvalidVersion
        );
    }

    #[test]
    fn decode_response_version_3_accepted() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[0] = (3 << 3) | 4; // VN=3, mode=4
        assert!(decode_response(&pkt).is_ok(), "version 3 must be accepted");
    }

    #[test]
    fn decode_response_stratum_zero_kiss_of_death() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[1] = 0; // stratum 0 = kiss-of-death
        assert_eq!(decode_response(&pkt).unwrap_err(), DecodeError::InvalidData);
    }

    #[test]
    fn decode_response_stratum_2_accepted() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[1] = 2; // stratum 2 is valid
        assert!(decode_response(&pkt).is_ok());
    }

    // ---- decode_packet (full struct decode) --------------------------------

    #[test]
    fn decode_packet_roundtrip_fields() {
        let ntp_secs: u32 = 3_900_000_000;
        let pkt = make_valid_response(ntp_secs, 0x12345678);
        let p = decode_packet(&pkt).unwrap();
        assert_eq!(p.stratum, 1);
        assert_eq!(p.poll, 6);
        assert_eq!(p.transmit_ts.seconds, ntp_secs);
        assert_eq!(p.transmit_ts.fraction, 0x12345678);
    }

    #[test]
    fn decode_packet_too_short() {
        assert_eq!(
            decode_packet(&[0u8; 10]).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
    }

    #[test]
    fn decode_packet_precision_signed() {
        let mut pkt = make_valid_response(3_900_000_000, 0);
        pkt[3] = 0xEC; // -20 as i8 (two's complement)
        let p = decode_packet(&pkt).unwrap();
        assert_eq!(p.precision, -20i8);
    }

    // ---- encode → decode roundtrip -----------------------------------------

    #[test]
    fn encode_decode_roundtrip() {
        // Encode a request
        let mut buf = [0u8; 48];
        let n = encode_request(&mut buf);
        assert_eq!(n, 48);

        // The request must be parseable (though a request won't pass
        // decode_response since it has stratum=0, we can decode_packet it)
        // Let's instead verify the packet structure directly:
        let version = (buf[0] >> 3) & 0x07;
        let mode = buf[0] & 0x07;
        assert_eq!(version, 4);
        assert_eq!(mode, 3);

        // Simulate a server response by copying the request buffer and
        // mutating it to look like a valid response
        let mut resp = buf;
        resp[0] = (4 << 3) | 4; // VN=4, mode=4 (server)
        resp[1] = 2; // stratum 2

        // Set transmit timestamp to a known value
        let ts_secs: u32 = 3_900_000_042;
        resp[40] = (ts_secs >> 24) as u8;
        resp[41] = (ts_secs >> 16) as u8;
        resp[42] = (ts_secs >> 8) as u8;
        resp[43] = ts_secs as u8;

        let ts = decode_response(&resp).unwrap();
        assert_eq!(ts.seconds, ts_secs);
    }

    // ---- ntp_to_unix_epoch -------------------------------------------------

    #[test]
    fn ntp_to_unix_epoch_known_value() {
        // NTP epoch 3_600_000_000 → Unix = 3_600_000_000 - 2_208_988_800 = 1_391_011_200
        // 1_391_011_200 is 2014-01-30 UTC — sanity check only
        let unix = ntp_to_unix_epoch(3_600_000_000).unwrap();
        assert_eq!(unix, 3_600_000_000u32 - NTP_UNIX_OFFSET);
    }

    #[test]
    fn ntp_to_unix_epoch_at_unix_epoch() {
        // NTP offset exactly → Unix epoch 0
        let unix = ntp_to_unix_epoch(NTP_UNIX_OFFSET).unwrap();
        assert_eq!(unix, 0);
    }

    #[test]
    fn ntp_to_unix_epoch_before_unix_epoch_returns_none() {
        // Any NTP value before the offset represents a date before 1970-01-01
        let result = ntp_to_unix_epoch(NTP_UNIX_OFFSET - 1);
        assert!(result.is_none(), "NTP secs before 1970 must return None");
    }

    #[test]
    fn ntp_to_unix_epoch_zero_returns_none() {
        assert!(ntp_to_unix_epoch(0).is_none());
    }

    #[test]
    fn ntp_to_unix_epoch_max_u32() {
        // NTP seconds = u32::MAX = 4_294_967_295
        // 4_294_967_295 - 2_208_988_800 = 2_085_978_495 (year ~2036)
        let unix = ntp_to_unix_epoch(u32::MAX).unwrap();
        assert_eq!(unix, u32::MAX - NTP_UNIX_OFFSET);
    }

    // ---- NtpTimestamp equality --------------------------------------------

    #[test]
    fn ntp_timestamp_equality() {
        let a = NtpTimestamp {
            seconds: 100,
            fraction: 200,
        };
        let b = NtpTimestamp {
            seconds: 100,
            fraction: 200,
        };
        assert_eq!(a, b);
        let c = NtpTimestamp {
            seconds: 100,
            fraction: 201,
        };
        assert_ne!(a, c);
    }

    // ---- Constant sanity checks -------------------------------------------

    #[test]
    fn ntp_unix_offset_correct() {
        // 70 years from 1900 to 1970: 17 leap years in range [1904, 1968].
        // 70*365*86400 + 17*86400 = 2_208_988_800
        assert_eq!(NTP_UNIX_OFFSET, 2_208_988_800u32);
    }

    #[test]
    fn sntp_packet_len_is_48() {
        assert_eq!(SNTP_PACKET_LEN, 48);
    }

    #[test]
    fn ntp_port_is_123() {
        assert_eq!(NTP_PORT, 123);
    }
}
