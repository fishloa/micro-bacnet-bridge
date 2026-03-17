//! BACnet NPDU (Network Protocol Data Unit) encode/decode.
//!
//! Reference: ASHRAE 135-2020, clause 6.

use crate::error::{DecodeError, EncodeError};

/// BACnet NPDU version. Always 0x01.
pub const NPDU_VERSION: u8 = 0x01;

// Control byte bit masks
/// Bit 7: NSDU contains a network-layer message (not APDU).
const CTRL_NET_LAYER_MSG: u8 = 0x80;
/// Bit 5: Destination specifier present.
const CTRL_DEST_PRESENT: u8 = 0x20;
/// Bit 3: Source specifier present.
const CTRL_SRC_PRESENT: u8 = 0x08;
/// Bit 2: Expecting reply.
const CTRL_EXPECTING_REPLY: u8 = 0x04;
/// Bits 1–0: Network priority (0 = normal, 3 = life-safety).
const CTRL_PRIORITY_MASK: u8 = 0x03;

/// Maximum length of a MS/TP / BACnet/IP MAC address in bytes.
pub const MAX_MAC_LEN: usize = 7;

/// Parsed BACnet NPDU header.
#[derive(Debug, Clone, PartialEq)]
pub struct NpduHeader {
    /// Always 1.
    pub version: u8,
    /// If true, this NSDU contains a network-layer message rather than an APDU.
    pub is_network_layer_msg: bool,
    /// If true, destination network/MAC are present.
    pub dest_present: bool,
    /// Destination network number (0 = local network).
    pub dest_net: u16,
    /// Destination MAC address bytes.
    pub dest_mac: [u8; MAX_MAC_LEN],
    /// Number of valid bytes in `dest_mac` (0 means broadcast on dest_net).
    pub dest_mac_len: u8,
    /// If true, source network/MAC are present.
    pub src_present: bool,
    /// Source network number.
    pub src_net: u16,
    /// Source MAC address bytes.
    pub src_mac: [u8; MAX_MAC_LEN],
    /// Number of valid bytes in `src_mac`.
    pub src_mac_len: u8,
    /// Hop count (only meaningful when dest is present).
    pub hop_count: u8,
    /// Whether the sender expects a reply.
    pub expecting_reply: bool,
    /// Network message priority (0–3).
    pub priority: u8,
}

impl NpduHeader {
    /// Create a simple local-broadcast NPDU header (no dest/src specifiers).
    pub fn local(expecting_reply: bool) -> Self {
        Self {
            version: NPDU_VERSION,
            is_network_layer_msg: false,
            dest_present: false,
            dest_net: 0,
            dest_mac: [0u8; MAX_MAC_LEN],
            dest_mac_len: 0,
            src_present: false,
            src_net: 0,
            src_mac: [0u8; MAX_MAC_LEN],
            src_mac_len: 0,
            hop_count: 0,
            expecting_reply,
            priority: 0,
        }
    }
}

/// Encode an NPDU header + APDU payload into `buf`.
///
/// Returns the number of bytes written.
pub fn encode_npdu(header: &NpduHeader, apdu: &[u8], buf: &mut [u8]) -> Result<usize, EncodeError> {
    let mut ctrl: u8 = 0;
    if header.is_network_layer_msg {
        ctrl |= CTRL_NET_LAYER_MSG;
    }
    if header.dest_present {
        ctrl |= CTRL_DEST_PRESENT;
    }
    if header.src_present {
        ctrl |= CTRL_SRC_PRESENT;
    }
    if header.expecting_reply {
        ctrl |= CTRL_EXPECTING_REPLY;
    }
    ctrl |= header.priority & CTRL_PRIORITY_MASK;

    // Calculate minimum required size
    let mut needed = 2; // version + control
    if header.dest_present {
        needed += 2 + 1 + header.dest_mac_len as usize; // dnet + dlen + dmac
    }
    if header.src_present {
        needed += 2 + 1 + header.src_mac_len as usize; // snet + slen + smac
    }
    if header.dest_present {
        needed += 1; // hop count
    }
    needed += apdu.len();

    if buf.len() < needed {
        return Err(EncodeError::BufferTooSmall);
    }

    let mut pos = 0;
    buf[pos] = NPDU_VERSION;
    pos += 1;
    buf[pos] = ctrl;
    pos += 1;

    if header.dest_present {
        buf[pos] = (header.dest_net >> 8) as u8;
        pos += 1;
        buf[pos] = header.dest_net as u8;
        pos += 1;
        let dlen = header.dest_mac_len as usize;
        buf[pos] = dlen as u8;
        pos += 1;
        buf[pos..pos + dlen].copy_from_slice(&header.dest_mac[..dlen]);
        pos += dlen;
    }

    if header.src_present {
        buf[pos] = (header.src_net >> 8) as u8;
        pos += 1;
        buf[pos] = header.src_net as u8;
        pos += 1;
        let slen = header.src_mac_len as usize;
        buf[pos] = slen as u8;
        pos += 1;
        buf[pos..pos + slen].copy_from_slice(&header.src_mac[..slen]);
        pos += slen;
    }

    if header.dest_present {
        buf[pos] = header.hop_count;
        pos += 1;
    }

    buf[pos..pos + apdu.len()].copy_from_slice(apdu);
    pos += apdu.len();

    Ok(pos)
}

/// Decode an NPDU from `data`.
///
/// Returns the parsed header and a slice pointing at the APDU portion.
pub fn decode_npdu(data: &[u8]) -> Result<(NpduHeader, &[u8]), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::UnexpectedEnd);
    }

    let version = data[0];
    if version != NPDU_VERSION {
        return Err(DecodeError::InvalidVersion);
    }

    let ctrl = data[1];
    let is_network_layer_msg = (ctrl & CTRL_NET_LAYER_MSG) != 0;
    let dest_present = (ctrl & CTRL_DEST_PRESENT) != 0;
    let src_present = (ctrl & CTRL_SRC_PRESENT) != 0;
    let expecting_reply = (ctrl & CTRL_EXPECTING_REPLY) != 0;
    let priority = ctrl & CTRL_PRIORITY_MASK;

    let mut pos = 2usize;
    let mut header = NpduHeader {
        version,
        is_network_layer_msg,
        dest_present,
        dest_net: 0,
        dest_mac: [0u8; MAX_MAC_LEN],
        dest_mac_len: 0,
        src_present,
        src_net: 0,
        src_mac: [0u8; MAX_MAC_LEN],
        src_mac_len: 0,
        hop_count: 0,
        expecting_reply,
        priority,
    };

    if dest_present {
        if data.len() < pos + 3 {
            return Err(DecodeError::UnexpectedEnd);
        }
        header.dest_net = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        pos += 2;
        let dlen = data[pos] as usize;
        pos += 1;
        if dlen > MAX_MAC_LEN {
            return Err(DecodeError::InvalidData);
        }
        if data.len() < pos + dlen {
            return Err(DecodeError::UnexpectedEnd);
        }
        header.dest_mac[..dlen].copy_from_slice(&data[pos..pos + dlen]);
        header.dest_mac_len = dlen as u8;
        pos += dlen;
    }

    if src_present {
        if data.len() < pos + 3 {
            return Err(DecodeError::UnexpectedEnd);
        }
        header.src_net = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        pos += 2;
        let slen = data[pos] as usize;
        pos += 1;
        if slen > MAX_MAC_LEN {
            return Err(DecodeError::InvalidData);
        }
        if data.len() < pos + slen {
            return Err(DecodeError::UnexpectedEnd);
        }
        header.src_mac[..slen].copy_from_slice(&data[pos..pos + slen]);
        header.src_mac_len = slen as u8;
        pos += slen;
    }

    if dest_present {
        if data.len() < pos + 1 {
            return Err(DecodeError::UnexpectedEnd);
        }
        header.hop_count = data[pos];
        pos += 1;
    }

    Ok((header, &data[pos..]))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a simple local header with no src/dest specifiers.
    fn local_header() -> NpduHeader {
        NpduHeader::local(false)
    }

    #[test]
    fn encode_decode_local_no_apdu() {
        let hdr = local_header();
        let apdu: &[u8] = &[];
        let mut buf = [0u8; 64];
        let n = encode_npdu(&hdr, apdu, &mut buf).unwrap();
        assert_eq!(n, 2);
        let (decoded, apdu_slice) = decode_npdu(&buf[..n]).unwrap();
        assert_eq!(decoded.version, NPDU_VERSION);
        assert!(!decoded.dest_present);
        assert!(!decoded.src_present);
        assert!(!decoded.expecting_reply);
        assert_eq!(apdu_slice, &[] as &[u8]);
    }

    #[test]
    fn encode_decode_with_apdu() {
        let hdr = NpduHeader {
            expecting_reply: true,
            ..NpduHeader::local(false)
        };
        let apdu = [0x10u8, 0x08, 0x00, 0x01];
        let mut buf = [0u8; 64];
        let n = encode_npdu(&hdr, &apdu, &mut buf).unwrap();
        let (decoded, apdu_out) = decode_npdu(&buf[..n]).unwrap();
        assert!(decoded.expecting_reply);
        assert_eq!(apdu_out, &apdu[..]);
    }

    #[test]
    fn encode_decode_with_dest() {
        let mut hdr = local_header();
        hdr.dest_present = true;
        hdr.dest_net = 0x0203;
        hdr.dest_mac = [0x01, 0x02, 0x03, 0x04, 0x05, 0x00, 0x00];
        hdr.dest_mac_len = 5;
        hdr.hop_count = 0xFF;

        let apdu = [0xABu8, 0xCD];
        let mut buf = [0u8; 64];
        let n = encode_npdu(&hdr, &apdu, &mut buf).unwrap();
        let (decoded, apdu_out) = decode_npdu(&buf[..n]).unwrap();

        assert!(decoded.dest_present);
        assert_eq!(decoded.dest_net, 0x0203);
        assert_eq!(decoded.dest_mac_len, 5);
        assert_eq!(&decoded.dest_mac[..5], &[0x01, 0x02, 0x03, 0x04, 0x05]);
        assert_eq!(decoded.hop_count, 0xFF);
        assert_eq!(apdu_out, &[0xABu8, 0xCD]);
    }

    #[test]
    fn encode_decode_with_src_and_dest() {
        let mut hdr = local_header();
        hdr.dest_present = true;
        hdr.dest_net = 1;
        hdr.dest_mac = [0xFF, 0, 0, 0, 0, 0, 0];
        hdr.dest_mac_len = 1;
        hdr.hop_count = 8;
        hdr.src_present = true;
        hdr.src_net = 2;
        hdr.src_mac = [0x05, 0, 0, 0, 0, 0, 0];
        hdr.src_mac_len = 1;

        let apdu = [0x30u8, 0x00, 0x0C];
        let mut buf = [0u8; 128];
        let n = encode_npdu(&hdr, &apdu, &mut buf).unwrap();
        let (decoded, apdu_out) = decode_npdu(&buf[..n]).unwrap();

        assert!(decoded.dest_present);
        assert!(decoded.src_present);
        assert_eq!(decoded.dest_net, 1);
        assert_eq!(decoded.src_net, 2);
        assert_eq!(decoded.dest_mac[0], 0xFF);
        assert_eq!(decoded.src_mac[0], 0x05);
        assert_eq!(apdu_out, &apdu[..]);
    }

    #[test]
    fn decode_known_who_is_packet() {
        // Version=1, ctrl=0x04 (expecting reply), service=WhoIs unconfirmed (0x10 0x08)
        let packet: &[u8] = &[0x01, 0x04, 0x10, 0x08];
        let (hdr, apdu) = decode_npdu(packet).unwrap();
        assert_eq!(hdr.version, 1);
        assert!(hdr.expecting_reply);
        assert!(!hdr.dest_present);
        assert_eq!(apdu, &[0x10u8, 0x08]);
    }

    #[test]
    fn decode_too_short() {
        assert_eq!(
            decode_npdu(&[0x01]).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
        assert_eq!(decode_npdu(&[]).unwrap_err(), DecodeError::UnexpectedEnd);
    }

    #[test]
    fn decode_wrong_version() {
        let packet = &[0x02u8, 0x00];
        assert_eq!(
            decode_npdu(packet).unwrap_err(),
            DecodeError::InvalidVersion
        );
    }

    #[test]
    fn encode_buffer_too_small() {
        let hdr = local_header();
        let apdu = [0u8; 10];
        let mut buf = [0u8; 4]; // too small for 2-byte header + 10-byte apdu
        assert_eq!(
            encode_npdu(&hdr, &apdu, &mut buf).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    #[test]
    fn priority_and_flags_round_trip() {
        let mut hdr = local_header();
        hdr.priority = 3;
        hdr.is_network_layer_msg = true;
        let mut buf = [0u8; 16];
        let n = encode_npdu(&hdr, &[], &mut buf).unwrap();
        let (decoded, _) = decode_npdu(&buf[..n]).unwrap();
        assert_eq!(decoded.priority, 3);
        assert!(decoded.is_network_layer_msg);
    }
}
