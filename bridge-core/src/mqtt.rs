//! MQTT 3.1.1 client codec (publish-only, no heap allocation).
//!
//! Implements encoding for the subset of MQTT 3.1.1 packets needed by a
//! publish-only client: CONNECT, PUBLISH, PINGREQ, and DISCONNECT. Decoding
//! covers the CONNACK packet so the caller can confirm the broker accepted the
//! connection.
//!
//! All functions write into a caller-supplied `&mut [u8]` buffer and return the
//! number of bytes written, or [`EncodeError::BufferTooSmall`] if the buffer is
//! not large enough. No dynamic allocation is performed anywhere.
//!
//! # Home Assistant auto-discovery
//!
//! [`format_ha_discovery`] produces the JSON payload and [`ha_discovery_topic`]
//! produces the MQTT topic for Home Assistant MQTT auto-discovery (2023.x+
//! format). Sensor objects use component type `sensor`; binary objects
//! (binary-input, binary-output, binary-value) use `binary_sensor`.
//!
//! # MQTT remaining-length encoding
//!
//! MQTT uses a variable-length encoding for the remaining-length field:
//! - 0–127    → 1 byte
//! - 128–16383 → 2 bytes (MSB continuation bit set on byte 0)
//! - 16384–2097151 → 3 bytes
//! - 2097152–268435455 → 4 bytes
//!
//! # References
//! - [MQTT 3.1.1 spec](http://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html)
//! - [Home Assistant MQTT discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery)

use crate::error::{DecodeError, EncodeError};
use heapless::String;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default MQTT broker port.
pub const MQTT_PORT: u16 = 1883;

// MQTT control packet types (upper nibble of the first byte).
/// CONNECT packet type.
pub const PACKET_TYPE_CONNECT: u8 = 1;
/// CONNACK packet type.
pub const PACKET_TYPE_CONNACK: u8 = 2;
/// PUBLISH packet type.
pub const PACKET_TYPE_PUBLISH: u8 = 3;
/// PINGREQ packet type.
pub const PACKET_TYPE_PINGREQ: u8 = 12;
/// PINGRESP packet type.
pub const PACKET_TYPE_PINGRESP: u8 = 13;
/// DISCONNECT packet type.
pub const PACKET_TYPE_DISCONNECT: u8 = 14;

// CONNECT flags
const CONNECT_FLAG_CLEAN_SESSION: u8 = 0x02;
const CONNECT_FLAG_PASSWORD: u8 = 0x40;
const CONNECT_FLAG_USERNAME: u8 = 0x80;

// CONNACK return codes
/// CONNACK: connection accepted.
pub const CONNACK_ACCEPTED: u8 = 0x00;

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

/// Write a single byte into `buf[pos]`, advancing `pos`.
///
/// Returns `Err(BufferTooSmall)` if there is no space.
#[inline]
fn write_u8(buf: &mut [u8], pos: &mut usize, val: u8) -> Result<(), EncodeError> {
    if *pos >= buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[*pos] = val;
    *pos += 1;
    Ok(())
}

/// Write a big-endian u16 into `buf[pos..]`, advancing `pos` by 2.
#[inline]
fn write_u16(buf: &mut [u8], pos: &mut usize, val: u16) -> Result<(), EncodeError> {
    if *pos + 2 > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[*pos] = (val >> 8) as u8;
    buf[*pos + 1] = (val & 0xFF) as u8;
    *pos += 2;
    Ok(())
}

/// Write a raw byte slice into `buf[pos..]`, advancing `pos`.
#[inline]
fn write_bytes(buf: &mut [u8], pos: &mut usize, data: &[u8]) -> Result<(), EncodeError> {
    let end = pos
        .checked_add(data.len())
        .ok_or(EncodeError::BufferTooSmall)?;
    if end > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[*pos..end].copy_from_slice(data);
    *pos = end;
    Ok(())
}

/// Write an MQTT length-prefixed string (2-byte big-endian length + UTF-8 bytes).
#[inline]
fn write_mqtt_str(buf: &mut [u8], pos: &mut usize, s: &str) -> Result<(), EncodeError> {
    let bytes = s.as_bytes();
    if bytes.len() > 0xFFFF {
        return Err(EncodeError::StringTooLong);
    }
    write_u16(buf, pos, bytes.len() as u16)?;
    write_bytes(buf, pos, bytes)
}

/// Write an MQTT variable-length remaining-length field.
///
/// Uses the standard MQTT multi-byte encoding (7 bits per byte, continuation
/// bit in the MSB). Returns `Err(InvalidValue)` if `len` exceeds 268,435,455.
fn write_remaining_length(
    buf: &mut [u8],
    pos: &mut usize,
    mut len: usize,
) -> Result<(), EncodeError> {
    if len > 268_435_455 {
        return Err(EncodeError::InvalidValue);
    }
    loop {
        let mut byte = (len & 0x7F) as u8;
        len >>= 7;
        if len > 0 {
            byte |= 0x80;
        }
        write_u8(buf, pos, byte)?;
        if len == 0 {
            break;
        }
    }
    Ok(())
}

/// Return how many bytes are needed to encode `len` as an MQTT remaining-length.
fn remaining_length_encoded_size(len: usize) -> usize {
    if len < 128 {
        1
    } else if len < 16_384 {
        2
    } else if len < 2_097_152 {
        3
    } else {
        4
    }
}

/// Read one byte from `data` at `*pos`, advancing `*pos`.
#[inline]
fn read_u8(data: &[u8], pos: &mut usize) -> Result<u8, DecodeError> {
    if *pos >= data.len() {
        return Err(DecodeError::UnexpectedEnd);
    }
    let b = data[*pos];
    *pos += 1;
    Ok(b)
}

/// Decode an MQTT variable-length remaining-length field.
///
/// Returns `(remaining_length, new_pos)`.
fn decode_remaining_length(data: &[u8], pos: &mut usize) -> Result<usize, DecodeError> {
    let mut multiplier: usize = 1;
    let mut value: usize = 0;
    loop {
        let byte = read_u8(data, pos)?;
        value = value
            .checked_add((byte & 0x7F) as usize * multiplier)
            .ok_or(DecodeError::InvalidData)?;
        if byte & 0x80 == 0 {
            break;
        }
        // MQTT spec §2.2.3: remaining length is at most 4 bytes.
        // After processing 4 bytes with the MSB set the multiplier would be
        // 128^4; if we're about to exceed that limit, the packet is malformed.
        multiplier = multiplier
            .checked_mul(128)
            .ok_or(DecodeError::InvalidData)?;
        if multiplier >= 128 * 128 * 128 * 128 {
            return Err(DecodeError::InvalidData);
        }
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode an MQTT 3.1.1 CONNECT packet.
///
/// - `client_id`: Client identifier string (1–23 characters recommended).
/// - `keep_alive`: Keep-alive interval in seconds (0 = disabled).
/// - `username`: Optional username; sets the USERNAME flag when `Some`.
/// - `password`: Optional password; sets the PASSWORD flag when `Some` and
///   requires `username` to be `Some` per the spec (caller responsibility).
///
/// Clean-session is always set to 1. QoS 0 is used (no will).
///
/// Returns the number of bytes written to `buf`.
pub fn encode_connect(
    buf: &mut [u8],
    client_id: &str,
    keep_alive: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<usize, EncodeError> {
    // --- Compute variable header + payload length ---
    // Variable header: protocol name (6) + protocol level (1) + connect flags (1) +
    //                  keep alive (2) = 10 bytes
    let vh_len: usize = 10;

    // Payload: client_id (2 + len), optional username (2 + len), optional password (2 + len)
    let client_id_bytes = client_id.as_bytes();
    let mut payload_len = 2 + client_id_bytes.len();
    if let Some(u) = username {
        payload_len += 2 + u.as_bytes().len();
    }
    if let Some(p) = password {
        payload_len += 2 + p.as_bytes().len();
    }

    let remaining = vh_len + payload_len;
    let header_size = 1 + remaining_length_encoded_size(remaining);
    let total = header_size + remaining;

    if total > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }

    let mut pos = 0;

    // Fixed header: packet type CONNECT (1 << 4), no flags
    write_u8(buf, &mut pos, PACKET_TYPE_CONNECT << 4)?;
    write_remaining_length(buf, &mut pos, remaining)?;

    // Variable header: protocol name "MQTT"
    write_u16(buf, &mut pos, 4)?; // length of "MQTT"
    write_bytes(buf, &mut pos, b"MQTT")?;
    write_u8(buf, &mut pos, 4)?; // Protocol level 4 = MQTT 3.1.1

    // Connect flags
    let mut flags = CONNECT_FLAG_CLEAN_SESSION;
    if username.is_some() {
        flags |= CONNECT_FLAG_USERNAME;
    }
    if password.is_some() {
        flags |= CONNECT_FLAG_PASSWORD;
    }
    write_u8(buf, &mut pos, flags)?;

    // Keep-alive
    write_u16(buf, &mut pos, keep_alive)?;

    // Payload: client ID
    write_mqtt_str(buf, &mut pos, client_id)?;

    // Payload: optional username
    if let Some(u) = username {
        write_mqtt_str(buf, &mut pos, u)?;
    }

    // Payload: optional password
    if let Some(p) = password {
        write_mqtt_str(buf, &mut pos, p)?;
    }

    Ok(pos)
}

/// Encode an MQTT 3.1.1 PUBLISH packet (QoS 0).
///
/// QoS 0 is used (fire-and-forget); therefore no packet identifier is included.
/// The `retain` flag controls the RETAIN bit in the fixed header flags byte.
///
/// Returns the number of bytes written to `buf`.
pub fn encode_publish(
    buf: &mut [u8],
    topic: &str,
    payload: &[u8],
    retain: bool,
) -> Result<usize, EncodeError> {
    let topic_bytes = topic.as_bytes();
    if topic_bytes.len() > 0xFFFF {
        return Err(EncodeError::StringTooLong);
    }

    // Remaining length = topic length field (2) + topic bytes + payload
    let remaining = 2 + topic_bytes.len() + payload.len();
    let header_size = 1 + remaining_length_encoded_size(remaining);
    let total = header_size + remaining;

    if total > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }

    let mut pos = 0;

    // Fixed header byte: packet type PUBLISH (3 << 4) | DUP=0 | QoS=0 | RETAIN
    let first_byte = (PACKET_TYPE_PUBLISH << 4) | if retain { 0x01 } else { 0x00 };
    write_u8(buf, &mut pos, first_byte)?;
    write_remaining_length(buf, &mut pos, remaining)?;

    // Variable header: topic name (length-prefixed)
    write_u16(buf, &mut pos, topic_bytes.len() as u16)?;
    write_bytes(buf, &mut pos, topic_bytes)?;

    // Payload (no packet identifier for QoS 0)
    write_bytes(buf, &mut pos, payload)?;

    Ok(pos)
}

/// Encode an MQTT 3.1.1 PINGREQ packet.
///
/// PINGREQ has a fixed two-byte encoding: `0xC0 0x00`.
///
/// Returns the number of bytes written (always 2 on success).
pub fn encode_pingreq(buf: &mut [u8]) -> Result<usize, EncodeError> {
    if buf.len() < 2 {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[0] = PACKET_TYPE_PINGREQ << 4;
    buf[1] = 0x00;
    Ok(2)
}

/// Encode an MQTT 3.1.1 DISCONNECT packet.
///
/// DISCONNECT has a fixed two-byte encoding: `0xE0 0x00`.
///
/// Returns the number of bytes written (always 2 on success).
pub fn encode_disconnect(buf: &mut [u8]) -> Result<usize, EncodeError> {
    if buf.len() < 2 {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[0] = PACKET_TYPE_DISCONNECT << 4;
    buf[1] = 0x00;
    Ok(2)
}

/// Decode the packet type and remaining length from the first bytes of `data`.
///
/// Returns `(packet_type, remaining_length)` where `packet_type` is the upper
/// nibble of the first byte (1–14 for standard MQTT packet types).
///
/// The caller should slice `data[(1 + encoded_remaining_len_bytes)..]` to get
/// the rest of the packet.
pub fn decode_packet_type(data: &[u8]) -> Result<(u8, usize), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::UnexpectedEnd);
    }
    let packet_type = (data[0] >> 4) & 0x0F;
    let mut pos = 1;
    let remaining = decode_remaining_length(data, &mut pos)?;
    Ok((packet_type, remaining))
}

/// Decode an MQTT 3.1.1 CONNACK packet.
///
/// Returns `true` if the broker accepted the connection (return code 0x00),
/// `false` if it was rejected (return codes 1–5), or `Err` if the packet is
/// malformed.
pub fn decode_connack(data: &[u8]) -> Result<bool, DecodeError> {
    // Minimum CONNACK: 0x20 0x02 <ack flags> <return code> = 4 bytes
    if data.len() < 4 {
        return Err(DecodeError::UnexpectedEnd);
    }
    // First byte: packet type CONNACK (2) in upper nibble
    if (data[0] >> 4) != PACKET_TYPE_CONNACK {
        return Err(DecodeError::InvalidData);
    }
    // Second byte: remaining length must be 2
    if data[1] != 2 {
        return Err(DecodeError::InvalidData);
    }
    // data[2]: connect acknowledge flags (bit 0 = session present)
    // data[3]: connect return code
    let return_code = data[3];
    Ok(return_code == CONNACK_ACCEPTED)
}

// ---------------------------------------------------------------------------
// Home Assistant auto-discovery
// ---------------------------------------------------------------------------

/// Return `true` if `object_type` corresponds to a binary HA component.
///
/// Binary objects (binary-input, binary-output, binary-value) map to the
/// `binary_sensor` HA component type; all others map to `sensor`.
fn is_binary_object(object_type: &str) -> bool {
    matches!(
        object_type,
        "binary-input" | "binary-output" | "binary-value"
    )
}

/// Write the HA discovery MQTT topic into `buf`.
///
/// Topic format: `{discovery_prefix}/{component}/{device_name}_{object_type}_{instance}/config`
///
/// For example, `homeassistant/sensor/bacnet-bridge_analog-input_0/config`.
///
/// Returns `Err(EncodeError::StringTooLong)` if the result does not fit in the
/// 128-character [`heapless::String`].
pub fn ha_discovery_topic(
    buf: &mut String<128>,
    discovery_prefix: &str,
    device_name: &str,
    object_type: &str,
    object_instance: u32,
) -> Result<(), EncodeError> {
    use core::fmt::Write;
    let component = if is_binary_object(object_type) {
        "binary_sensor"
    } else {
        "sensor"
    };
    write!(
        buf,
        "{}/{}/{}_{}_{}/config",
        discovery_prefix, component, device_name, object_type, object_instance
    )
    .map_err(|_| EncodeError::StringTooLong)
}

/// Format the Home Assistant MQTT discovery JSON payload into `buf`.
///
/// The JSON follows the HA 2023.x MQTT discovery schema:
/// - `name`: human-readable point name
/// - `unique_id`: `bacnet_{device_name}_{object_type}_{instance}` (underscores)
/// - `state_topic`: caller-supplied topic string
/// - `unit_of_measurement`: unit label (omitted for binary sensors)
/// - `device`: identifiers, name, manufacturer (Icomb Place)
///
/// Binary objects use component `binary_sensor` (no `unit_of_measurement` key).
///
/// Returns the number of bytes written to `buf` on success.
pub fn format_ha_discovery(
    buf: &mut [u8],
    discovery_prefix: &str,
    device_name: &str,
    point_name: &str,
    object_type: &str,
    object_instance: u32,
    unit: &str,
    state_topic: &str,
) -> Result<usize, EncodeError> {
    // Build unique_id: replace hyphens with underscores for a valid identifier.
    // We write it directly into a stack-allocated scratch buffer.
    let _ = discovery_prefix; // used via ha_discovery_topic; present for API symmetry

    let binary = is_binary_object(object_type);

    // We need a scratch String<64> for unique_id and object_type (underscored).
    let mut unique_id: String<64> = String::new();
    {
        use core::fmt::Write;
        // Replace hyphens in object_type with underscores for the unique_id.
        write!(unique_id, "bacnet_{}_", device_name).map_err(|_| EncodeError::StringTooLong)?;
        for ch in object_type.chars() {
            let c = if ch == '-' { '_' } else { ch };
            unique_id.push(c).map_err(|_| EncodeError::StringTooLong)?;
        }
        write!(unique_id, "_{}", object_instance).map_err(|_| EncodeError::StringTooLong)?;
    }

    // Write the JSON directly into `buf` using a simple cursor.
    let mut pos = 0usize;

    macro_rules! w {
        ($s:expr) => {
            write_bytes(buf, &mut pos, $s.as_bytes())
        };
    }

    w!(r#"{"name":""#)?;
    write_bytes(buf, &mut pos, point_name.as_bytes())?;
    w!(r#"","unique_id":""#)?;
    write_bytes(buf, &mut pos, unique_id.as_bytes())?;
    w!(r#"","state_topic":""#)?;
    write_bytes(buf, &mut pos, state_topic.as_bytes())?;
    w!("\"")?;

    if !binary {
        w!(r#","unit_of_measurement":""#)?;
        write_bytes(buf, &mut pos, unit.as_bytes())?;
        w!("\"")?;
    }

    w!(r#","device":{"identifiers":[""#)?;
    write_bytes(buf, &mut pos, device_name.as_bytes())?;
    w!(r#""],"name":"BACnet Bridge","manufacturer":"Icomb Place"}}"#)?;

    Ok(pos)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Remaining-length encoding helpers
    // -----------------------------------------------------------------------

    #[test]
    fn remaining_length_encoding_single_byte() {
        // Values 0–127 must encode in exactly 1 byte.
        let mut buf = [0u8; 4];
        let mut pos = 0;
        write_remaining_length(&mut buf, &mut pos, 0).unwrap();
        assert_eq!(pos, 1);
        assert_eq!(buf[0], 0x00);

        pos = 0;
        write_remaining_length(&mut buf, &mut pos, 127).unwrap();
        assert_eq!(pos, 1);
        assert_eq!(buf[0], 0x7F);
    }

    #[test]
    fn remaining_length_encoding_two_bytes() {
        // 128 encodes as 0x80 0x01 (2 bytes).
        let mut buf = [0u8; 4];
        let mut pos = 0;
        write_remaining_length(&mut buf, &mut pos, 128).unwrap();
        assert_eq!(pos, 2);
        assert_eq!(&buf[..2], &[0x80, 0x01]);

        // 16383 encodes as 0xFF 0x7F.
        pos = 0;
        write_remaining_length(&mut buf, &mut pos, 16383).unwrap();
        assert_eq!(pos, 2);
        assert_eq!(&buf[..2], &[0xFF, 0x7F]);
    }

    #[test]
    fn remaining_length_encoding_three_bytes() {
        // 16384 encodes as 0x80 0x80 0x01.
        let mut buf = [0u8; 4];
        let mut pos = 0;
        write_remaining_length(&mut buf, &mut pos, 16384).unwrap();
        assert_eq!(pos, 3);
        assert_eq!(&buf[..3], &[0x80, 0x80, 0x01]);
    }

    // -----------------------------------------------------------------------
    // CONNECT
    // -----------------------------------------------------------------------

    #[test]
    fn encode_connect_minimal() {
        let mut buf = [0u8; 64];
        let n = encode_connect(&mut buf, "test-client", 60, None, None).unwrap();

        // First byte: 0x10 = CONNECT packet type (1 << 4)
        assert_eq!(buf[0], 0x10);

        // Find "MQTT" magic in the variable header
        let pkt = &buf[..n];
        let mqtt_pos = pkt.windows(4).position(|w| w == b"MQTT").unwrap();
        assert!(mqtt_pos > 0);

        // Protocol level byte immediately after "MQTT" must be 4
        assert_eq!(pkt[mqtt_pos + 4], 4);

        // Connect flags: clean session only (0x02), no username/password
        assert_eq!(pkt[mqtt_pos + 5], CONNECT_FLAG_CLEAN_SESSION);

        // Packet must contain the client ID string
        assert!(pkt.windows(11).any(|w| w == b"test-client"));
    }

    #[test]
    fn encode_connect_with_auth() {
        let mut buf = [0u8; 128];
        let n = encode_connect(&mut buf, "bridge-01", 30, Some("admin"), Some("secret")).unwrap();
        let pkt = &buf[..n];

        // Locate connect flags (byte after protocol level 4)
        let mqtt_pos = pkt.windows(4).position(|w| w == b"MQTT").unwrap();
        let flags = pkt[mqtt_pos + 5];

        // USERNAME and PASSWORD flags must be set
        assert_ne!(flags & CONNECT_FLAG_USERNAME, 0);
        assert_ne!(flags & CONNECT_FLAG_PASSWORD, 0);

        // Strings must appear in packet body
        assert!(pkt.windows(5).any(|w| w == b"admin"));
        assert!(pkt.windows(6).any(|w| w == b"secret"));
    }

    #[test]
    fn encode_connect_buffer_too_small() {
        let mut buf = [0u8; 4]; // far too small
        let result = encode_connect(&mut buf, "client", 60, None, None);
        assert_eq!(result, Err(EncodeError::BufferTooSmall));
    }

    // -----------------------------------------------------------------------
    // PUBLISH
    // -----------------------------------------------------------------------

    #[test]
    fn encode_publish_qos0() {
        let mut buf = [0u8; 128];
        let topic = "bacnet-bridge/analog-input/0/state";
        let payload = b"21.5";
        let n = encode_publish(&mut buf, topic, payload, false).unwrap();
        let pkt = &buf[..n];

        // First byte: 0x30 = PUBLISH, QoS 0, no retain, no dup
        assert_eq!(pkt[0], 0x30);

        // Topic must appear length-prefixed in the packet
        let topic_len = topic.len() as u16;
        let tl_bytes = topic_len.to_be_bytes();
        let pos = pkt.windows(2).position(|w| w == tl_bytes).unwrap();
        assert_eq!(&pkt[pos + 2..pos + 2 + topic.len()], topic.as_bytes());

        // Payload must appear at the end
        assert!(pkt.ends_with(payload));
    }

    #[test]
    fn encode_publish_retain() {
        let mut buf = [0u8; 64];
        let n = encode_publish(&mut buf, "bridge/status", b"online", true).unwrap();

        // Retain bit is bit 0 of first byte
        assert_ne!(buf[0] & 0x01, 0, "RETAIN bit must be set");
        assert!(buf[..n].ends_with(b"online"));
    }

    #[test]
    fn encode_publish_buffer_too_small() {
        let mut buf = [0u8; 4];
        let result = encode_publish(&mut buf, "a/b", b"value", false);
        assert_eq!(result, Err(EncodeError::BufferTooSmall));
    }

    // -----------------------------------------------------------------------
    // PINGREQ / DISCONNECT
    // -----------------------------------------------------------------------

    #[test]
    fn encode_pingreq_packet() {
        let mut buf = [0u8; 4];
        let n = encode_pingreq(&mut buf).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], &[0xC0, 0x00]);
    }

    #[test]
    fn encode_disconnect_packet() {
        let mut buf = [0u8; 4];
        let n = encode_disconnect(&mut buf).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], &[0xE0, 0x00]);
    }

    #[test]
    fn encode_pingreq_buffer_too_small() {
        let mut buf = [0u8; 1];
        assert_eq!(encode_pingreq(&mut buf), Err(EncodeError::BufferTooSmall));
    }

    #[test]
    fn encode_disconnect_buffer_too_small() {
        let mut buf = [0u8; 1];
        assert_eq!(
            encode_disconnect(&mut buf),
            Err(EncodeError::BufferTooSmall)
        );
    }

    // -----------------------------------------------------------------------
    // decode_packet_type
    // -----------------------------------------------------------------------

    #[test]
    fn decode_packet_type_connack() {
        // Well-formed CONNACK: 0x20 0x02 0x00 0x00
        let data = [0x20u8, 0x02, 0x00, 0x00];
        let (ptype, remaining) = decode_packet_type(&data).unwrap();
        assert_eq!(ptype, PACKET_TYPE_CONNACK);
        assert_eq!(remaining, 2);
    }

    #[test]
    fn decode_packet_type_publish() {
        // PUBLISH with remaining length 5
        let data = [0x30u8, 0x05, 0x00, 0x03, b'a', b'/', b'b', 0x42];
        let (ptype, remaining) = decode_packet_type(&data).unwrap();
        assert_eq!(ptype, PACKET_TYPE_PUBLISH);
        assert_eq!(remaining, 5);
    }

    // -----------------------------------------------------------------------
    // decode_connack
    // -----------------------------------------------------------------------

    #[test]
    fn decode_connack_accepted() {
        // 0x20 0x02 <ack-flags=0x00> <return-code=0x00>
        let data = [0x20u8, 0x02, 0x00, 0x00];
        assert_eq!(decode_connack(&data), Ok(true));
    }

    #[test]
    fn decode_connack_rejected() {
        // Return code 0x05 = not authorized
        let data = [0x20u8, 0x02, 0x00, 0x05];
        assert_eq!(decode_connack(&data), Ok(false));
    }

    #[test]
    fn decode_connack_bad_packet_type() {
        // First byte 0x10 = CONNECT, not CONNACK
        let data = [0x10u8, 0x02, 0x00, 0x00];
        assert_eq!(decode_connack(&data), Err(DecodeError::InvalidData));
    }

    #[test]
    fn decode_connack_too_short() {
        let data = [0x20u8, 0x02, 0x00];
        assert_eq!(decode_connack(&data), Err(DecodeError::UnexpectedEnd));
    }

    // -----------------------------------------------------------------------
    // Home Assistant discovery
    // -----------------------------------------------------------------------

    #[test]
    fn ha_discovery_sensor() {
        let mut buf = [0u8; 512];
        let n = format_ha_discovery(
            &mut buf,
            "homeassistant",
            "bacnet-bridge",
            "Supply Air Temp",
            "analog-input",
            0,
            "°C",
            "bacnet-bridge/analog-input/0/state",
        )
        .unwrap();

        let json = core::str::from_utf8(&buf[..n]).unwrap();

        // Required fields
        assert!(json.contains(r#""name":"Supply Air Temp""#));
        assert!(json.contains(r#""unique_id":"bacnet_bacnet-bridge_analog_input_0""#));
        assert!(json.contains(r#""state_topic":"bacnet-bridge/analog-input/0/state""#));
        assert!(json.contains(r#""unit_of_measurement":"°C""#));
        assert!(json.contains(r#""manufacturer":"Icomb Place""#));
        assert!(json.contains(r#""identifiers":["bacnet-bridge"]"#));
        assert!(json.contains(r#""name":"BACnet Bridge""#));
    }

    #[test]
    fn ha_discovery_binary_sensor() {
        let mut buf = [0u8; 512];
        let n = format_ha_discovery(
            &mut buf,
            "homeassistant",
            "bacnet-bridge",
            "Occupied",
            "binary-input",
            3,
            "",
            "bacnet-bridge/binary-input/3/state",
        )
        .unwrap();

        let json = core::str::from_utf8(&buf[..n]).unwrap();

        // Binary sensor must NOT include unit_of_measurement
        assert!(!json.contains("unit_of_measurement"));
        assert!(json.contains(r#""name":"Occupied""#));
        assert!(json.contains(r#""unique_id":"bacnet_bacnet-bridge_binary_input_3""#));
        assert!(json.contains(r#""manufacturer":"Icomb Place""#));
    }

    #[test]
    fn ha_discovery_topic_sensor() {
        let mut topic: String<128> = String::new();
        ha_discovery_topic(
            &mut topic,
            "homeassistant",
            "bacnet-bridge",
            "analog-input",
            0,
        )
        .unwrap();
        assert_eq!(
            topic.as_str(),
            "homeassistant/sensor/bacnet-bridge_analog-input_0/config"
        );
    }

    #[test]
    fn ha_discovery_topic_binary() {
        let mut topic: String<128> = String::new();
        ha_discovery_topic(
            &mut topic,
            "homeassistant",
            "bacnet-bridge",
            "binary-output",
            5,
        )
        .unwrap();
        assert_eq!(
            topic.as_str(),
            "homeassistant/binary_sensor/bacnet-bridge_binary-output_5/config"
        );
    }

    // -----------------------------------------------------------------------
    // Round-trip: publish a discovery payload via encode_publish
    // -----------------------------------------------------------------------

    #[test]
    fn publish_ha_discovery_roundtrip() {
        // Build the payload
        let mut payload_buf = [0u8; 512];
        let payload_len = format_ha_discovery(
            &mut payload_buf,
            "homeassistant",
            "bacnet-bridge",
            "Supply Air Temp",
            "analog-input",
            0,
            "°C",
            "bacnet-bridge/analog-input/0/state",
        )
        .unwrap();

        // Build the topic
        let mut topic: String<128> = String::new();
        ha_discovery_topic(
            &mut topic,
            "homeassistant",
            "bacnet-bridge",
            "analog-input",
            0,
        )
        .unwrap();

        // Encode the PUBLISH packet
        let mut pkt_buf = [0u8; 700];
        let n = encode_publish(
            &mut pkt_buf,
            topic.as_str(),
            &payload_buf[..payload_len],
            true,
        )
        .unwrap();

        // Verify it looks like a PUBLISH with retain
        assert_eq!(pkt_buf[0], 0x31); // 0x30 | RETAIN bit
        assert!(n > payload_len);
    }

    // -----------------------------------------------------------------------
    // Regression: decode_remaining_length must reject 5-byte sequences
    // (MQTT spec §2.2.3 limits remaining-length to 4 bytes)
    // -----------------------------------------------------------------------

    /// A 5-byte continuation sequence (all high bits set) must return
    /// `InvalidData`, not loop and read a fifth byte.
    ///
    /// Regression: the original check `multiplier > 128^4` used `>` instead
    /// of `>=`, so after reading 4 MSB-set bytes the multiplier equalled
    /// 128^4 exactly and the check was `128^4 > 128^4` = false, allowing
    /// a fifth byte to be consumed.
    #[test]
    fn decode_remaining_length_rejects_5_byte_sequence() {
        // PUBLISH packet with 5 continuation bytes in the remaining-length field.
        // 0xFF 0xFF 0xFF 0xFF 0x01 — the 4th byte still has MSB set (0xFF).
        let malformed = [0x30u8, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00];
        let result = decode_packet_type(&malformed);
        assert_eq!(result, Err(DecodeError::InvalidData));
    }

    /// 4-byte max remaining-length (268,435,455 = 0x0FFFFFFF) must decode
    /// correctly: 0xFF 0xFF 0xFF 0x7F.
    #[test]
    fn decode_remaining_length_max_4_byte_value() {
        // Fixed header byte (PUBLISH = 0x30) + 4-byte remaining length + 1 payload
        // 0xFF 0xFF 0xFF 0x7F = 268,435,455
        let data = [0x30u8, 0xFF, 0xFF, 0xFF, 0x7F];
        let (ptype, remaining) = decode_packet_type(&data).unwrap();
        assert_eq!(ptype, PACKET_TYPE_PUBLISH);
        assert_eq!(remaining, 268_435_455);
    }

    /// Single-byte max (127) must not read a second byte.
    #[test]
    fn decode_remaining_length_max_single_byte() {
        let data = [0x30u8, 0x7F]; // PUBLISH, remaining=127
        let (ptype, remaining) = decode_packet_type(&data).unwrap();
        assert_eq!(ptype, PACKET_TYPE_PUBLISH);
        assert_eq!(remaining, 127);
    }
}
