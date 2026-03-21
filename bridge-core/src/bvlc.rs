//! BACnet Virtual Link Control (BVLC) for BACnet/IP.
//!
//! Reference: ASHRAE 135-2020, Annex J.
//!
//! BVLC wraps NPDU+APDU in a 4-byte header for transport over UDP port 47808.
//! This module handles the two most common function codes:
//! Original-Unicast-NPDU and Original-Broadcast-NPDU.

use crate::error::{DecodeError, EncodeError};

/// BACnet/IP type byte (always 0x81).
pub const BVLC_TYPE: u8 = 0x81;

/// BACnet/IP UDP port.
pub const BACNET_IP_PORT: u16 = 0xBAC0; // 47808

// BVLC function codes
/// Original-Unicast-NPDU (Annex J.4.3).
pub const BVLC_ORIGINAL_UNICAST: u8 = 0x0A;
/// Original-Broadcast-NPDU (Annex J.4.4).
pub const BVLC_ORIGINAL_BROADCAST: u8 = 0x0B;
/// Forwarded-NPDU (from BBMD, Annex J.4.1).
pub const BVLC_FORWARDED_NPDU: u8 = 0x04;
/// Register-Foreign-Device (Annex J.5.2).
pub const BVLC_REGISTER_FOREIGN: u8 = 0x05;
/// Result (Annex J.4.1).
pub const BVLC_RESULT: u8 = 0x00;
/// Distribute-Broadcast-To-Network (Annex J.4.5).
pub const BVLC_DISTRIBUTE_BROADCAST: u8 = 0x09;

/// BVLC header size in bytes (type + function + 2-byte length).
pub const BVLC_HEADER_SIZE: usize = 4;

/// Forwarded-NPDU has 6 extra bytes (4-byte IP + 2-byte port) after the header.
pub const FORWARDED_EXTRA: usize = 6;

/// Parsed BVLC header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BvlcHeader {
    /// Function code (e.g. `BVLC_ORIGINAL_UNICAST`).
    pub function: u8,
    /// Total packet length including the 4-byte BVLC header.
    pub length: u16,
}

/// Encode a BVLC header + NPDU payload into `buf`.
///
/// `function` should be `BVLC_ORIGINAL_UNICAST` or `BVLC_ORIGINAL_BROADCAST`.
/// Returns the total number of bytes written.
pub fn encode_bvlc(function: u8, npdu: &[u8], buf: &mut [u8]) -> Result<usize, EncodeError> {
    let total = BVLC_HEADER_SIZE + npdu.len();
    if total > u16::MAX as usize {
        return Err(EncodeError::InvalidValue);
    }
    if buf.len() < total {
        return Err(EncodeError::BufferTooSmall);
    }

    buf[0] = BVLC_TYPE;
    buf[1] = function;
    buf[2] = (total >> 8) as u8;
    buf[3] = total as u8;
    buf[BVLC_HEADER_SIZE..total].copy_from_slice(npdu);

    Ok(total)
}

/// Decode a BVLC packet.
///
/// Returns the header and a slice of the NPDU payload.
/// For Forwarded-NPDU packets, the 6-byte forwarding address is skipped
/// and only the NPDU is returned.
pub fn decode_bvlc(data: &[u8]) -> Result<(BvlcHeader, &[u8]), DecodeError> {
    if data.len() < BVLC_HEADER_SIZE {
        return Err(DecodeError::UnexpectedEnd);
    }
    if data[0] != BVLC_TYPE {
        return Err(DecodeError::InvalidVersion);
    }

    let function = data[1];
    let length = ((data[2] as u16) << 8) | (data[3] as u16);

    if (length as usize) > data.len() {
        return Err(DecodeError::LengthOutOfBounds);
    }

    let header = BvlcHeader { function, length };

    // For Forwarded-NPDU, skip the 6-byte originating address.
    let npdu_start = if function == BVLC_FORWARDED_NPDU {
        if (length as usize) < BVLC_HEADER_SIZE + FORWARDED_EXTRA {
            return Err(DecodeError::UnexpectedEnd);
        }
        BVLC_HEADER_SIZE + FORWARDED_EXTRA
    } else {
        BVLC_HEADER_SIZE
    };

    Ok((header, &data[npdu_start..length as usize]))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_unicast() {
        let npdu = [0x01, 0x04, 0x10, 0x08]; // version + ctrl + Who-Is
        let mut buf = [0u8; 64];
        let n = encode_bvlc(BVLC_ORIGINAL_UNICAST, &npdu, &mut buf).unwrap();
        assert_eq!(n, 8); // 4 header + 4 NPDU
        assert_eq!(buf[0], BVLC_TYPE);
        assert_eq!(buf[1], BVLC_ORIGINAL_UNICAST);
        assert_eq!(buf[2], 0x00);
        assert_eq!(buf[3], 0x08);

        let (hdr, payload) = decode_bvlc(&buf[..n]).unwrap();
        assert_eq!(hdr.function, BVLC_ORIGINAL_UNICAST);
        assert_eq!(hdr.length, 8);
        assert_eq!(payload, &npdu);
    }

    #[test]
    fn encode_decode_broadcast() {
        let npdu = [0x01, 0x20, 0xFF, 0xFF, 0x00, 0xFF, 0x10, 0x08];
        let mut buf = [0u8; 64];
        let n = encode_bvlc(BVLC_ORIGINAL_BROADCAST, &npdu, &mut buf).unwrap();
        let (hdr, payload) = decode_bvlc(&buf[..n]).unwrap();
        assert_eq!(hdr.function, BVLC_ORIGINAL_BROADCAST);
        assert_eq!(payload, &npdu);
    }

    #[test]
    fn decode_forwarded_npdu() {
        // Forwarded-NPDU: 4-byte header + 6-byte address + NPDU
        let mut pkt = [0u8; 14];
        pkt[0] = BVLC_TYPE;
        pkt[1] = BVLC_FORWARDED_NPDU;
        pkt[2] = 0x00;
        pkt[3] = 14; // total length
                     // 6-byte forwarding address (IP + port)
        pkt[4..10].copy_from_slice(&[192, 168, 1, 100, 0xBA, 0xC0]);
        // NPDU
        pkt[10..14].copy_from_slice(&[0x01, 0x00, 0x10, 0x08]);

        let (hdr, payload) = decode_bvlc(&pkt).unwrap();
        assert_eq!(hdr.function, BVLC_FORWARDED_NPDU);
        assert_eq!(payload, &[0x01, 0x00, 0x10, 0x08]);
    }

    #[test]
    fn decode_too_short() {
        assert!(decode_bvlc(&[0x81, 0x0A, 0x00]).is_err());
        assert!(decode_bvlc(&[]).is_err());
    }

    #[test]
    fn decode_bad_type() {
        assert_eq!(
            decode_bvlc(&[0x82, 0x0A, 0x00, 0x04]).unwrap_err(),
            DecodeError::InvalidVersion
        );
    }

    #[test]
    fn decode_length_exceeds_data() {
        // Claims length 10 but only 4 bytes provided
        assert_eq!(
            decode_bvlc(&[0x81, 0x0A, 0x00, 0x0A]).unwrap_err(),
            DecodeError::LengthOutOfBounds
        );
    }

    #[test]
    fn encode_buffer_too_small() {
        let npdu = [0u8; 10];
        let mut buf = [0u8; 8]; // need 14
        assert_eq!(
            encode_bvlc(BVLC_ORIGINAL_UNICAST, &npdu, &mut buf).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    #[test]
    fn empty_npdu() {
        let mut buf = [0u8; 4];
        let n = encode_bvlc(BVLC_ORIGINAL_UNICAST, &[], &mut buf).unwrap();
        assert_eq!(n, 4);
        let (hdr, payload) = decode_bvlc(&buf[..n]).unwrap();
        assert_eq!(hdr.length, 4);
        assert!(payload.is_empty());
    }

    #[test]
    fn known_wireshark_packet() {
        // Real BACnet/IP Who-Is broadcast captured from Wireshark
        let pkt: &[u8] = &[
            0x81, 0x0B, 0x00, 0x08, // BVLC: broadcast, length 8
            0x01, 0x04, 0x10, 0x08, // NPDU: version 1, expecting reply + Who-Is
        ];
        let (hdr, npdu) = decode_bvlc(pkt).unwrap();
        assert_eq!(hdr.function, BVLC_ORIGINAL_BROADCAST);
        assert_eq!(npdu, &[0x01, 0x04, 0x10, 0x08]);
    }
}
