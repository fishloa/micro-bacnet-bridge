//! Minimal SNMP v2c agent codec (RFC 3416).
//!
//! Implements just enough ASN.1 BER encoding/decoding to handle SNMP v2c
//! GetRequest and GetNextRequest PDUs and produce GetResponse PDUs. All
//! operations use caller-supplied buffers — no heap allocation.
//!
//! # Scope
//! - ASN.1 BER primitive TLV encoding/decoding for Integer, OctetString, OID,
//!   Null, and Sequence types.
//! - SNMP v2c-specific types: Counter32, Gauge32, TimeTicks.
//! - SNMP message structure: `Sequence { version, community, PDU }`.
//! - PDU structure: `GetRequest-PDU` / `GetNextRequest-PDU` / `GetResponse-PDU`.
//! - Read-only agent: SetRequest handling is left to the caller (ignored).
//!
//! # Not in scope
//! - SNMPv3 (USM, encryption, HMAC).
//! - Large SNMP messages (> 1472 bytes UDP payload).
//! - BER length encoding for lengths > 127 bytes in indefinite form.
//! - OID encoding beyond 16 sub-identifiers or sub-IDs > 2^28.

use crate::error::{DecodeError, EncodeError};
use heapless::Vec;

// ---------------------------------------------------------------------------
// ASN.1 / BER tag constants
// ---------------------------------------------------------------------------

/// ASN.1 Universal SEQUENCE tag (constructed, 0x30).
pub const TAG_SEQUENCE: u8 = 0x30;
/// ASN.1 Universal INTEGER tag (primitive, 0x02).
pub const TAG_INTEGER: u8 = 0x02;
/// ASN.1 Universal OCTET STRING tag (primitive, 0x04).
pub const TAG_OCTET_STRING: u8 = 0x04;
/// ASN.1 Universal NULL tag (primitive, 0x05).
pub const TAG_NULL: u8 = 0x05;
/// ASN.1 Universal OBJECT IDENTIFIER tag (primitive, 0x06).
pub const TAG_OID: u8 = 0x06;

// SNMP application-class tags (per RFC 1902)
/// SNMP Counter32 (application 1, primitive).
pub const TAG_COUNTER32: u8 = 0x41;
/// SNMP Gauge32 (application 2, primitive).
pub const TAG_GAUGE32: u8 = 0x42;
/// SNMP TimeTicks (application 3, primitive).
pub const TAG_TIMETICKS: u8 = 0x43;

// SNMP PDU context tags (context, constructed)
/// SNMP GetRequest-PDU tag (context [0] constructed = 0xA0).
pub const TAG_GET_REQUEST: u8 = 0xA0;
/// SNMP GetNextRequest-PDU tag (context [1] constructed = 0xA1).
pub const TAG_GET_NEXT_REQUEST: u8 = 0xA1;
/// SNMP GetResponse-PDU tag (context [2] constructed = 0xA2).
pub const TAG_GET_RESPONSE: u8 = 0xA2;

// SNMP error-status codes
/// No error.
pub const ERROR_NO_ERROR: i32 = 0;
/// Object not found in MIB.
pub const ERROR_NO_SUCH_NAME: i32 = 2;
/// General error.
pub const ERROR_GEN_ERR: i32 = 5;

// ---------------------------------------------------------------------------
// OID constants for System MIB (RFC 1213 / RFC 3418)
// ---------------------------------------------------------------------------

/// sysDescr (1.3.6.1.2.1.1.1.0)
pub const OID_SYS_DESCR: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 1, 0];
/// sysObjectID (1.3.6.1.2.1.1.2.0)
pub const OID_SYS_OBJECT_ID: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 2, 0];
/// sysUpTime (1.3.6.1.2.1.1.3.0)
pub const OID_SYS_UPTIME: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 3, 0];
/// sysContact (1.3.6.1.2.1.1.4.0)
pub const OID_SYS_CONTACT: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 4, 0];
/// sysName (1.3.6.1.2.1.1.5.0)
pub const OID_SYS_NAME: &[u32] = &[1, 3, 6, 1, 2, 1, 1, 5, 0];

// Enterprise OID base: iso.org.dod.internet.private.enterprises.99999
// 1.3.6.1.4.1.99999
/// Custom: mstpFramesSent (1.3.6.1.4.1.99999.1.1)
pub const OID_MSTP_FRAMES_SENT: &[u32] = &[1, 3, 6, 1, 4, 1, 99999, 1, 1];
/// Custom: mstpFramesRecv (1.3.6.1.4.1.99999.1.2)
pub const OID_MSTP_FRAMES_RECV: &[u32] = &[1, 3, 6, 1, 4, 1, 99999, 1, 2];
/// Custom: mstpTokenLosses (1.3.6.1.4.1.99999.1.3)
pub const OID_MSTP_TOKEN_LOSSES: &[u32] = &[1, 3, 6, 1, 4, 1, 99999, 1, 3];
/// Custom: ipcDropCount (1.3.6.1.4.1.99999.1.4)
pub const OID_IPC_DROP_COUNT: &[u32] = &[1, 3, 6, 1, 4, 1, 99999, 1, 4];
/// Custom: bacnetDevicesDiscovered (1.3.6.1.4.1.99999.1.5)
pub const OID_BACNET_DEVICES_DISCOVERED: &[u32] = &[1, 3, 6, 1, 4, 1, 99999, 1, 5];

// ---------------------------------------------------------------------------
// SNMP value type
// ---------------------------------------------------------------------------

/// A typed SNMP variable-binding value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnmpValue {
    /// ASN.1 INTEGER (signed 32-bit).
    Integer(i32),
    /// ASN.1 OCTET STRING (up to 64 bytes).
    OctetString(Vec<u8, 64>),
    /// Counter32 (unsigned 32-bit, wraps).
    Counter32(u32),
    /// Gauge32 (unsigned 32-bit, clamps).
    Gauge32(u32),
    /// TimeTicks (hundredths of seconds since uptime = 0).
    TimeTicks(u32),
    /// ASN.1 NULL (used in request variable bindings).
    Null,
}

// ---------------------------------------------------------------------------
// VarBind
// ---------------------------------------------------------------------------

/// A single variable binding: an OID plus its value (or NULL for requests).
///
/// OIDs are stored as a heapless Vec of u32 sub-identifiers (up to 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarBind {
    /// The object identifier.
    pub oid: Vec<u32, 16>,
    /// The object value.
    pub value: SnmpValue,
}

// ---------------------------------------------------------------------------
// SnmpRequest
// ---------------------------------------------------------------------------

/// A decoded SNMP GetRequest or GetNextRequest PDU.
#[derive(Debug, Clone)]
pub struct SnmpRequest {
    /// SNMP version (0 = v1, 1 = v2c).
    pub version: i32,
    /// Community string (up to 32 bytes).
    pub community: Vec<u8, 32>,
    /// Request ID (from the PDU header).
    pub request_id: i32,
    /// PDU type tag: `TAG_GET_REQUEST` or `TAG_GET_NEXT_REQUEST`.
    pub pdu_type: u8,
    /// OIDs being requested (up to 8).
    pub oids: Vec<Vec<u32, 16>, 8>,
}

// ---------------------------------------------------------------------------
// Low-level BER TLV helpers
// ---------------------------------------------------------------------------

/// Decode one BER TLV from `data` starting at `pos`.
///
/// Returns `(tag, value_start, value_end, next_pos)`:
/// - `tag` — the tag byte
/// - `value_start` — byte offset of the first value byte
/// - `value_end` — byte offset just past the last value byte
/// - `next_pos` — byte offset just past the end of this TLV
///
/// Supports short-form length (≤ 127) and long-form length with up to 4
/// length octets.
pub fn decode_tlv(data: &[u8], pos: usize) -> Result<(u8, usize, usize, usize), DecodeError> {
    if pos >= data.len() {
        return Err(DecodeError::UnexpectedEnd);
    }
    let tag = data[pos];
    let pos = pos + 1;

    if pos >= data.len() {
        return Err(DecodeError::UnexpectedEnd);
    }
    let len_byte = data[pos];
    let pos = pos + 1;

    let (length, value_start) = if len_byte & 0x80 == 0 {
        // Short form
        (len_byte as usize, pos)
    } else {
        // Long form: low 7 bits = number of length octets
        let num_len_bytes = (len_byte & 0x7F) as usize;
        if num_len_bytes == 0 || num_len_bytes > 4 {
            return Err(DecodeError::LengthOutOfBounds);
        }
        if pos + num_len_bytes > data.len() {
            return Err(DecodeError::UnexpectedEnd);
        }
        let mut len: usize = 0;
        for i in 0..num_len_bytes {
            len = (len << 8) | (data[pos + i] as usize);
        }
        (len, pos + num_len_bytes)
    };

    let value_end = value_start
        .checked_add(length)
        .ok_or(DecodeError::LengthOutOfBounds)?;
    if value_end > data.len() {
        return Err(DecodeError::LengthOutOfBounds);
    }

    Ok((tag, value_start, value_end, value_end))
}

/// Decode a BER INTEGER from `data[value_start..value_end]` as an `i32`.
///
/// Supports 1–4 byte signed integers (big-endian, two's complement).
/// Returns `Err(InvalidData)` for zero-length or > 4-byte integers.
pub fn decode_integer(
    data: &[u8],
    value_start: usize,
    value_end: usize,
) -> Result<i32, DecodeError> {
    let len = value_end
        .checked_sub(value_start)
        .ok_or(DecodeError::InvalidData)?;
    if len == 0 || len > 4 {
        return Err(DecodeError::InvalidData);
    }
    let bytes = &data[value_start..value_end];
    // Sign-extend from the most-significant byte
    let mut val = if bytes[0] & 0x80 != 0 { -1i32 } else { 0i32 };
    for &b in bytes {
        val = (val << 8) | (b as i32);
    }
    Ok(val)
}

/// Decode a BER unsigned 32-bit integer from `data[value_start..value_end]`.
///
/// Used for Counter32, Gauge32, TimeTicks. Supports 1–5 bytes (the 5-byte
/// form has a leading 0x00 to avoid sign extension per BER).
pub fn decode_u32(data: &[u8], value_start: usize, value_end: usize) -> Result<u32, DecodeError> {
    let len = value_end
        .checked_sub(value_start)
        .ok_or(DecodeError::InvalidData)?;
    if len == 0 || len > 5 {
        return Err(DecodeError::InvalidData);
    }
    let bytes = &data[value_start..value_end];
    // Skip optional leading 0x00 (added by BER to prevent sign extension)
    let start = if len == 5 && bytes[0] == 0x00 { 1 } else { 0 };
    let effective = &bytes[start..];
    if effective.len() > 4 {
        return Err(DecodeError::InvalidData);
    }
    let mut val: u32 = 0;
    for &b in effective {
        val = (val << 8) | (b as u32);
    }
    Ok(val)
}

/// Decode a BER OBJECT IDENTIFIER from `data[value_start..value_end]`.
///
/// Returns the OID as a `heapless::Vec<u32, 16>`.
///
/// Decoding follows the BER OID encoding rules: the first octet encodes the
/// first two sub-identifiers as `40 * X + Y`; subsequent sub-identifiers are
/// encoded in base-128 big-endian with the high bit set on all but the last
/// octet of each sub-identifier.
pub fn decode_oid(
    data: &[u8],
    value_start: usize,
    value_end: usize,
) -> Result<Vec<u32, 16>, DecodeError> {
    let bytes = &data[value_start..value_end];
    if bytes.is_empty() {
        return Err(DecodeError::InvalidData);
    }

    let mut oid: Vec<u32, 16> = Vec::new();

    // First byte encodes two sub-identifiers: first = byte / 40, second = byte % 40
    // (with special handling for first sub-id >= 2)
    let first_byte = bytes[0] as u32;
    let (sub0, sub1) = if first_byte < 40 {
        (0u32, first_byte)
    } else if first_byte < 80 {
        (1u32, first_byte - 40)
    } else {
        (2u32, first_byte - 80)
    };
    oid.push(sub0).map_err(|_| DecodeError::InvalidData)?;
    oid.push(sub1).map_err(|_| DecodeError::InvalidData)?;

    // Remaining bytes: base-128 big-endian sub-identifiers
    let mut i = 1;
    while i < bytes.len() {
        let mut sub_id: u32 = 0;
        loop {
            if i >= bytes.len() {
                return Err(DecodeError::UnexpectedEnd);
            }
            let b = bytes[i];
            i += 1;
            // Check for overflow before shifting
            if sub_id > 0x00FF_FFFF {
                return Err(DecodeError::InvalidData);
            }
            sub_id = (sub_id << 7) | ((b & 0x7F) as u32);
            if b & 0x80 == 0 {
                break; // last octet of this sub-identifier
            }
        }
        oid.push(sub_id).map_err(|_| DecodeError::InvalidData)?;
    }

    Ok(oid)
}

// ---------------------------------------------------------------------------
// Low-level BER TLV encoding helpers
// ---------------------------------------------------------------------------

/// Encode a BER TLV header (tag + length) into `buf` at `pos`.
/// Returns the new `pos` after the header.
fn encode_tlv_header(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    length: usize,
) -> Result<usize, EncodeError> {
    if length <= 127 {
        // Short form
        if pos + 2 > buf.len() {
            return Err(EncodeError::BufferTooSmall);
        }
        buf[pos] = tag;
        buf[pos + 1] = length as u8;
        Ok(pos + 2)
    } else if length <= 0xFF {
        if pos + 3 > buf.len() {
            return Err(EncodeError::BufferTooSmall);
        }
        buf[pos] = tag;
        buf[pos + 1] = 0x81;
        buf[pos + 2] = length as u8;
        Ok(pos + 3)
    } else if length <= 0xFFFF {
        if pos + 4 > buf.len() {
            return Err(EncodeError::BufferTooSmall);
        }
        buf[pos] = tag;
        buf[pos + 1] = 0x82;
        buf[pos + 2] = (length >> 8) as u8;
        buf[pos + 3] = length as u8;
        Ok(pos + 4)
    } else {
        Err(EncodeError::InvalidValue)
    }
}

/// Encode a BER INTEGER (i32) into `buf` at `pos`.
///
/// The integer is encoded using the minimum number of bytes required by BER
/// (1 to 4 bytes). Returns `Err(BufferTooSmall)` if there is insufficient
/// space.
pub fn encode_integer(buf: &mut [u8], pos: usize, val: i32) -> Result<usize, EncodeError> {
    // Compute minimal byte representation (big-endian, signed, no redundant bytes)
    let bytes = val.to_be_bytes(); // [u8; 4]
                                   // Find the minimal number of bytes needed
    let len = minimal_signed_byte_len(&bytes);
    let pos = encode_tlv_header(buf, pos, TAG_INTEGER, len)?;
    if pos + len > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[pos..pos + len].copy_from_slice(&bytes[4 - len..]);
    Ok(pos + len)
}

/// Encode a BER OCTET STRING into `buf` at `pos`.
pub fn encode_octet_string(buf: &mut [u8], pos: usize, val: &[u8]) -> Result<usize, EncodeError> {
    let pos = encode_tlv_header(buf, pos, TAG_OCTET_STRING, val.len())?;
    if pos + val.len() > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[pos..pos + val.len()].copy_from_slice(val);
    Ok(pos + val.len())
}

/// Encode a BER NULL into `buf` at `pos`.
pub fn encode_null(buf: &mut [u8], pos: usize) -> Result<usize, EncodeError> {
    if pos + 2 > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[pos] = TAG_NULL;
    buf[pos + 1] = 0x00;
    Ok(pos + 2)
}

/// Encode a BER OBJECT IDENTIFIER into `buf` at `pos`.
///
/// The OID is encoded following the BER rules: the first two sub-identifiers
/// are combined into a single byte, and subsequent sub-identifiers are encoded
/// in base-128 big-endian.
///
/// Requires at least 2 sub-identifiers. Returns `Err(InvalidValue)` if the
/// OID has fewer than 2 components or sub-identifiers would overflow.
pub fn encode_oid(buf: &mut [u8], pos: usize, oid: &[u32]) -> Result<usize, EncodeError> {
    if oid.len() < 2 {
        return Err(EncodeError::InvalidValue);
    }

    // Compute OID content bytes into a temporary buffer
    let mut tmp = [0u8; 128];
    let mut tmp_pos = 0usize;

    // First byte: 40 * oid[0] + oid[1]
    let first = 40 * oid[0] + oid[1];
    let first_enc = encode_base128(first, &mut tmp, tmp_pos)?;
    tmp_pos = first_enc;

    for &sub in &oid[2..] {
        let after = encode_base128(sub, &mut tmp, tmp_pos)?;
        tmp_pos = after;
    }

    let content = &tmp[..tmp_pos];
    let pos = encode_tlv_header(buf, pos, TAG_OID, content.len())?;
    if pos + content.len() > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[pos..pos + content.len()].copy_from_slice(content);
    Ok(pos + content.len())
}

/// Encode a u32 sub-identifier in base-128 big-endian into `buf` at `pos`.
fn encode_base128(val: u32, buf: &mut [u8], pos: usize) -> Result<usize, EncodeError> {
    if val == 0 {
        if pos >= buf.len() {
            return Err(EncodeError::BufferTooSmall);
        }
        buf[pos] = 0x00;
        return Ok(pos + 1);
    }
    // Compute bytes in reverse
    let mut tmp = [0u8; 5];
    let mut n = 0usize;
    let mut v = val;
    while v > 0 {
        tmp[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
    }
    // Write in forward order with continuation bits
    if pos + n > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    for i in 0..n {
        buf[pos + i] = tmp[n - 1 - i] | if i < n - 1 { 0x80 } else { 0x00 };
    }
    Ok(pos + n)
}

/// Encode a SEQUENCE wrapping `inner_len` bytes that were already written
/// into `buf` starting at `inner_start`. This is the backfill pattern: the
/// caller writes the SEQUENCE contents to `buf[inner_start..]`, then calls
/// this function to prepend the tag + length in a provided header buffer.
///
/// More practically, this writes `tag + length` *before* `inner_start` in
/// `buf`. The caller must have reserved at least `header_size(inner_len)`
/// bytes before `inner_start`.
///
/// In this implementation we use a **front-reservation** approach: encode
/// the inner content at `pos + max_header`, then shift it left to `pos + actual_header`.
/// For simplicity, callers can also use `encode_sequence_header` and manage
/// the buffer themselves.
///
/// Writes `TAG SEQUENCE` + BER length at `pos`, where the content is
/// `buf[inner_start..inner_start + inner_len]`. Returns the total length
/// (header + inner).
pub fn encode_sequence_header(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    inner_len: usize,
) -> Result<usize, EncodeError> {
    encode_tlv_header(buf, pos, tag, inner_len)
}

/// Encode a 32-bit unsigned value with a given application tag (Counter32,
/// Gauge32, or TimeTicks).
pub fn encode_u32_app(buf: &mut [u8], pos: usize, tag: u8, val: u32) -> Result<usize, EncodeError> {
    // BER encoding of unsigned 32-bit: 4 bytes, or 5 if high bit set (to avoid sign)
    let bytes = val.to_be_bytes();
    let (start, len) = if bytes[0] & 0x80 != 0 {
        (4usize, 5usize) // prepend 0x00
    } else {
        // find minimal representation (but unsigned, so no sign extension needed)
        let zeros = bytes.iter().take_while(|&&b| b == 0).count();
        let min_len = 4usize.saturating_sub(zeros).max(1);
        (4 - min_len, min_len)
    };

    let pos = encode_tlv_header(buf, pos, tag, len)?;
    if pos + len > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    if len == 5 {
        buf[pos] = 0x00;
        buf[pos + 1..pos + 5].copy_from_slice(&bytes);
    } else {
        buf[pos..pos + len].copy_from_slice(&bytes[start..]);
    }
    Ok(pos + len)
}

// ---------------------------------------------------------------------------
// Higher-level SNMP message encoding
// ---------------------------------------------------------------------------

/// Encode a VarBind sequence into `buf` at `pos`.
///
/// A VarBind is: `SEQUENCE { OID, value }`.
pub fn encode_varbind(buf: &mut [u8], pos: usize, vb: &VarBind) -> Result<usize, EncodeError> {
    // We use the "write inner, then prepend header" approach with a two-pass write.
    // Reserve space for the outer SEQUENCE header (max 4 bytes for long-form length).
    const HDR_RESERVE: usize = 4;
    let inner_start = pos + HDR_RESERVE;

    // Encode OID
    let after_oid = encode_oid(buf, inner_start, vb.oid.as_slice())?;
    // Encode value
    let after_value = encode_snmp_value(buf, after_oid, &vb.value)?;

    let inner_len = after_value - inner_start;

    // Compute actual header size for this inner_len
    let actual_hdr_size = ber_header_size(inner_len);

    // Shift inner content left to immediately follow the actual header
    let final_start = pos + actual_hdr_size;
    if final_start < inner_start {
        // Shift left by (HDR_RESERVE - actual_hdr_size)
        let shift = inner_start - final_start;
        buf.copy_within(inner_start..after_value, inner_start - shift);
    }

    // Write the SEQUENCE header at pos
    encode_tlv_header(buf, pos, TAG_SEQUENCE, inner_len)?;

    Ok(pos + actual_hdr_size + inner_len)
}

/// Encode an SnmpValue into `buf` at `pos`.
fn encode_snmp_value(buf: &mut [u8], pos: usize, val: &SnmpValue) -> Result<usize, EncodeError> {
    match val {
        SnmpValue::Integer(v) => encode_integer(buf, pos, *v),
        SnmpValue::OctetString(s) => encode_octet_string(buf, pos, s.as_slice()),
        SnmpValue::Counter32(v) => encode_u32_app(buf, pos, TAG_COUNTER32, *v),
        SnmpValue::Gauge32(v) => encode_u32_app(buf, pos, TAG_GAUGE32, *v),
        SnmpValue::TimeTicks(v) => encode_u32_app(buf, pos, TAG_TIMETICKS, *v),
        SnmpValue::Null => encode_null(buf, pos),
    }
}

/// Encode a GetResponse-PDU into `buf`.
///
/// # Arguments
/// - `buf` — output buffer.
/// - `request_id` — copied from the GetRequest.
/// - `community` — community string (typically "public").
/// - `error_status` — 0 = no error, 2 = noSuchName, 5 = genErr.
/// - `error_index` — 0 unless error_status != 0.
/// - `bindings` — the variable bindings to include.
///
/// # Returns
/// Number of bytes written, or `Err(EncodeError)`.
pub fn encode_get_response(
    buf: &mut [u8],
    request_id: i32,
    community: &[u8],
    error_status: i32,
    error_index: i32,
    bindings: &[VarBind],
) -> Result<usize, EncodeError> {
    // We use a two-pass approach with a large scratch area.
    // Since buffers are at most ~1472 bytes in practice, this is manageable.
    // Structure: SEQUENCE { version, community, GetResponse-PDU { req_id, err_status, err_index, VarBindList } }

    // We build from the inside out into a scratch buffer, then wrap.
    // Use a fixed-size stack scratch buffer.
    const SCRATCH: usize = 1500;
    let mut scratch = [0u8; SCRATCH];
    let mut p = 0usize;

    // --- VarBindList ---
    let vbl_content_start = p;
    for vb in bindings {
        p = encode_varbind(&mut scratch, p, vb)?;
    }
    let vbl_content_end = p;
    let vbl_inner_len = vbl_content_end - vbl_content_start;

    // Wrap VarBindList in a SEQUENCE
    let vbl_hdr_size = ber_header_size(vbl_inner_len);
    // Shift content right to make room for the SEQUENCE header
    let vbl_total = vbl_hdr_size + vbl_inner_len;
    if vbl_content_start + vbl_total > SCRATCH {
        return Err(EncodeError::BufferTooSmall);
    }
    scratch.copy_within(
        vbl_content_start..vbl_content_end,
        vbl_content_start + vbl_hdr_size,
    );
    encode_tlv_header(&mut scratch, vbl_content_start, TAG_SEQUENCE, vbl_inner_len)?;
    p = vbl_content_start + vbl_total;

    // --- PDU body: req_id, err_status, err_index, VarBindList ---
    // We'll build PDU content into another scratch area working backwards...
    // Simplest approach: use a second pass buffer.
    let mut pdu_content = [0u8; SCRATCH];
    let mut pp = 0usize;
    pp = encode_integer(&mut pdu_content, pp, request_id)?;
    pp = encode_integer(&mut pdu_content, pp, error_status)?;
    pp = encode_integer(&mut pdu_content, pp, error_index)?;
    // Append VarBindList (already in scratch[vbl_content_start..p])
    let vbl_bytes = &scratch[vbl_content_start..p];
    if pp + vbl_bytes.len() > pdu_content.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    pdu_content[pp..pp + vbl_bytes.len()].copy_from_slice(vbl_bytes);
    pp += vbl_bytes.len();

    let pdu_inner_len = pp;
    let pdu_hdr_size = ber_header_size(pdu_inner_len);
    let _pdu_total = pdu_hdr_size + pdu_inner_len;

    // --- SNMP message: version, community, PDU ---
    let mut msg_content = [0u8; SCRATCH];
    let mut mp = 0usize;
    mp = encode_integer(&mut msg_content, mp, 1)?; // version = 1 (v2c)
    mp = encode_octet_string(&mut msg_content, mp, community)?;
    // Append PDU (tag + pdu_content)
    if mp + pdu_hdr_size + pdu_inner_len > msg_content.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    encode_tlv_header(&mut msg_content, mp, TAG_GET_RESPONSE, pdu_inner_len)?;
    mp += pdu_hdr_size;
    msg_content[mp..mp + pdu_inner_len].copy_from_slice(&pdu_content[..pdu_inner_len]);
    mp += pdu_inner_len;

    let msg_inner_len = mp;
    let msg_hdr_size = ber_header_size(msg_inner_len);
    let msg_total = msg_hdr_size + msg_inner_len;

    // --- Write final output ---
    if msg_total > buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    encode_tlv_header(buf, 0, TAG_SEQUENCE, msg_inner_len)?;
    buf[msg_hdr_size..msg_total].copy_from_slice(&msg_content[..msg_inner_len]);
    Ok(msg_total)
}

/// Decode an SNMP GetRequest or GetNextRequest from `data`.
///
/// Returns a parsed `SnmpRequest` on success.
///
/// # Errors
/// - `UnexpectedEnd` — packet is truncated.
/// - `InvalidVersion` — SNMP version is not 1 (v2c).
/// - `InvalidData` — structural error (bad tags, community too long, etc.).
pub fn decode_get_request(data: &[u8]) -> Result<SnmpRequest, DecodeError> {
    // Outer SEQUENCE
    let (tag, vs, ve, _) = decode_tlv(data, 0)?;
    if tag != TAG_SEQUENCE {
        return Err(DecodeError::InvalidData);
    }
    let msg = &data[vs..ve];
    let mut pos = 0usize;

    // version INTEGER
    let (tag, vvs, vve, next) = decode_tlv(msg, pos)?;
    if tag != TAG_INTEGER {
        return Err(DecodeError::InvalidData);
    }
    let version = decode_integer(msg, vvs, vve)?;
    if version != 1 {
        // We accept only v2c (version field = 1)
        return Err(DecodeError::InvalidVersion);
    }
    pos = next;

    // community OCTET STRING
    let (tag, cvs, cve, next) = decode_tlv(msg, pos)?;
    if tag != TAG_OCTET_STRING {
        return Err(DecodeError::InvalidData);
    }
    let comm_bytes = &msg[cvs..cve];
    if comm_bytes.len() > 32 {
        return Err(DecodeError::InvalidData);
    }
    let mut community: Vec<u8, 32> = Vec::new();
    for &b in comm_bytes {
        community.push(b).map_err(|_| DecodeError::InvalidData)?;
    }
    pos = next;

    // PDU: GetRequest-PDU or GetNextRequest-PDU
    let (pdu_tag, pvs, pve, _) = decode_tlv(msg, pos)?;
    if pdu_tag != TAG_GET_REQUEST && pdu_tag != TAG_GET_NEXT_REQUEST {
        return Err(DecodeError::InvalidData);
    }
    let pdu = &msg[pvs..pve];
    let mut pp = 0usize;

    // request-id INTEGER
    let (tag, rvs, rve, next) = decode_tlv(pdu, pp)?;
    if tag != TAG_INTEGER {
        return Err(DecodeError::InvalidData);
    }
    let request_id = decode_integer(pdu, rvs, rve)?;
    pp = next;

    // error-status INTEGER (skip — must be 0 in request)
    let (tag, _, _, next) = decode_tlv(pdu, pp)?;
    if tag != TAG_INTEGER {
        return Err(DecodeError::InvalidData);
    }
    pp = next;

    // error-index INTEGER (skip — must be 0 in request)
    let (tag, _, _, next) = decode_tlv(pdu, pp)?;
    if tag != TAG_INTEGER {
        return Err(DecodeError::InvalidData);
    }
    pp = next;

    // VarBindList SEQUENCE
    let (tag, vlvs, vlve, _) = decode_tlv(pdu, pp)?;
    if tag != TAG_SEQUENCE {
        return Err(DecodeError::InvalidData);
    }
    let vbl = &pdu[vlvs..vlve];
    let mut vp = 0usize;

    let mut oids: Vec<Vec<u32, 16>, 8> = Vec::new();
    while vp < vbl.len() {
        // VarBind SEQUENCE
        let (tag, vbvs, vbve, next) = decode_tlv(vbl, vp)?;
        if tag != TAG_SEQUENCE {
            return Err(DecodeError::InvalidData);
        }
        let vb = &vbl[vbvs..vbve];
        vp = next;

        // OID
        let (tag, ovs, ove, _) = decode_tlv(vb, 0)?;
        if tag != TAG_OID {
            return Err(DecodeError::InvalidData);
        }
        let oid = decode_oid(vb, ovs, ove)?;
        oids.push(oid).map_err(|_| DecodeError::InvalidData)?;
    }

    Ok(SnmpRequest {
        version,
        community,
        request_id,
        pdu_type: pdu_tag,
        oids,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Number of bytes required for a BER TLV header (tag + length) encoding
/// `inner_len` content bytes.
fn ber_header_size(inner_len: usize) -> usize {
    if inner_len <= 127 {
        2 // tag + 1 length byte
    } else if inner_len <= 255 {
        3 // tag + 0x81 + 1 byte
    } else {
        4 // tag + 0x82 + 2 bytes
    }
}

/// Compute the minimum number of bytes needed to represent `bytes` as a signed
/// BER integer (no leading redundant bytes).
fn minimal_signed_byte_len(bytes: &[u8; 4]) -> usize {
    // Trim leading bytes that are either all-zero (positive) or all-0xFF (negative)
    // but preserve the sign bit: we must keep at least one byte.
    let mut len = 4usize;
    while len > 1 {
        let hi = bytes[4 - len];
        let next = bytes[4 - len + 1];
        // Can trim this leading byte if it's 0x00 and next byte's sign bit is 0,
        // or if it's 0xFF and next byte's sign bit is 1
        if (hi == 0x00 && next & 0x80 == 0) || (hi == 0xFF && next & 0x80 != 0) {
            len -= 1;
        } else {
            break;
        }
    }
    len
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ASN.1 BER integer encoding ----------------------------------------

    #[test]
    fn encode_integer_zero() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, 0).unwrap();
        // 0x02 0x01 0x00
        assert_eq!(&buf[..n], &[0x02, 0x01, 0x00]);
    }

    #[test]
    fn encode_integer_one() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, 1).unwrap();
        assert_eq!(&buf[..n], &[0x02, 0x01, 0x01]);
    }

    #[test]
    fn encode_integer_minus_one() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, -1).unwrap();
        // -1 in BER is 0xFF (1 byte, two's complement)
        assert_eq!(&buf[..n], &[0x02, 0x01, 0xFF]);
    }

    #[test]
    fn encode_integer_127() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, 127).unwrap();
        assert_eq!(&buf[..n], &[0x02, 0x01, 0x7F]);
    }

    #[test]
    fn encode_integer_128() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, 128).unwrap();
        // 128 = 0x80 — needs 2 bytes to avoid sign extension: 0x00 0x80
        assert_eq!(&buf[..n], &[0x02, 0x02, 0x00, 0x80]);
    }

    #[test]
    fn encode_integer_minus_128() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, -128).unwrap();
        // -128 = 0x80 in one byte
        assert_eq!(&buf[..n], &[0x02, 0x01, 0x80]);
    }

    #[test]
    fn encode_integer_large_positive() {
        let mut buf = [0u8; 16];
        let n = encode_integer(&mut buf, 0, 0x00FF_0000).unwrap();
        // 3 bytes: 0xFF 0x00 0x00 — but 0xFF with no sign bit set requires prefix 0x00
        // Actually 0x00FF0000 = [0x00, 0xFF, 0x00, 0x00] — minimal is [0xFF, 0x00, 0x00] (3 bytes)
        // since 0xFF has high bit set, we need [0x00, 0xFF, 0x00, 0x00] → 4 bytes
        // Let's check: 0x00FF0000 as i32 = 16711680
        // The 4-byte be repr is [0x00, 0xFF, 0x00, 0x00]
        // minimal_signed: leading 0x00, next byte 0xFF has high bit set → can't trim → 4 bytes
        assert_eq!(n, 6); // tag(1) + len(1) + 4 bytes
        assert_eq!(buf[0], 0x02);
        assert_eq!(buf[1], 0x04);
    }

    #[test]
    fn encode_decode_integer_roundtrip() {
        for val in [
            -32768i32,
            -1,
            0,
            1,
            127,
            128,
            255,
            256,
            65535,
            100000,
            i32::MAX,
            i32::MIN,
        ] {
            let mut buf = [0u8; 16];
            let n = encode_integer(&mut buf, 0, val).unwrap();
            let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
            assert_eq!(tag, TAG_INTEGER);
            let decoded = decode_integer(&buf[..n], vs, ve).unwrap();
            assert_eq!(decoded, val, "roundtrip failed for {}", val);
        }
    }

    // ---- ASN.1 BER octet string encoding -----------------------------------

    #[test]
    fn encode_decode_octet_string() {
        let data = b"public";
        let mut buf = [0u8; 32];
        let n = encode_octet_string(&mut buf, 0, data).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_OCTET_STRING);
        assert_eq!(&buf[vs..ve], data);
    }

    #[test]
    fn encode_octet_string_empty() {
        let mut buf = [0u8; 8];
        let n = encode_octet_string(&mut buf, 0, b"").unwrap();
        assert_eq!(&buf[..n], &[0x04, 0x00]);
    }

    // ---- ASN.1 OID encoding and decoding -----------------------------------

    #[test]
    fn encode_decode_oid_sysuptime() {
        let oid = OID_SYS_UPTIME; // 1.3.6.1.2.1.1.3.0
        let mut buf = [0u8; 32];
        let n = encode_oid(&mut buf, 0, oid).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_OID);
        let decoded = decode_oid(&buf[..n], vs, ve).unwrap();
        assert_eq!(decoded.as_slice(), oid);
    }

    #[test]
    fn encode_decode_oid_enterprise() {
        // 1.3.6.1.4.1.99999.1.1
        let oid = OID_MSTP_FRAMES_SENT;
        let mut buf = [0u8; 32];
        let n = encode_oid(&mut buf, 0, oid).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_OID);
        let decoded = decode_oid(&buf[..n], vs, ve).unwrap();
        assert_eq!(decoded.as_slice(), oid);
    }

    #[test]
    fn encode_oid_too_short_returns_error() {
        let mut buf = [0u8; 32];
        // OID must have at least 2 sub-identifiers
        assert_eq!(
            encode_oid(&mut buf, 0, &[1u32]).unwrap_err(),
            EncodeError::InvalidValue
        );
    }

    #[test]
    fn encode_decode_oid_all_constants() {
        let oids: &[&[u32]] = &[
            OID_SYS_DESCR,
            OID_SYS_UPTIME,
            OID_SYS_CONTACT,
            OID_SYS_NAME,
            OID_MSTP_FRAMES_SENT,
            OID_MSTP_FRAMES_RECV,
            OID_MSTP_TOKEN_LOSSES,
            OID_IPC_DROP_COUNT,
            OID_BACNET_DEVICES_DISCOVERED,
        ];
        for &oid in oids {
            let mut buf = [0u8; 32];
            let n = encode_oid(&mut buf, 0, oid).unwrap();
            let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
            assert_eq!(tag, TAG_OID);
            let decoded = decode_oid(&buf[..n], vs, ve).unwrap();
            assert_eq!(
                decoded.as_slice(),
                oid,
                "OID roundtrip failed for {:?}",
                oid
            );
        }
    }

    // ---- u32 application types ---------------------------------------------

    #[test]
    fn encode_decode_counter32() {
        let mut buf = [0u8; 16];
        let n = encode_u32_app(&mut buf, 0, TAG_COUNTER32, 0xDEAD_BEEF).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_COUNTER32);
        let val = decode_u32(&buf[..n], vs, ve).unwrap();
        assert_eq!(val, 0xDEAD_BEEF);
    }

    #[test]
    fn encode_decode_timeticks_zero() {
        let mut buf = [0u8; 16];
        let n = encode_u32_app(&mut buf, 0, TAG_TIMETICKS, 0).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_TIMETICKS);
        let val = decode_u32(&buf[..n], vs, ve).unwrap();
        assert_eq!(val, 0);
    }

    #[test]
    fn encode_decode_gauge32_max() {
        let mut buf = [0u8; 16];
        let n = encode_u32_app(&mut buf, 0, TAG_GAUGE32, u32::MAX).unwrap();
        let (tag, vs, ve, _) = decode_tlv(&buf[..n], 0).unwrap();
        assert_eq!(tag, TAG_GAUGE32);
        let val = decode_u32(&buf[..n], vs, ve).unwrap();
        assert_eq!(val, u32::MAX);
    }

    // ---- decode_tlv --------------------------------------------------------

    #[test]
    fn decode_tlv_short_form() {
        let data = [0x02u8, 0x01, 0x05]; // INTEGER, length=1, value=5
        let (tag, vs, ve, next) = decode_tlv(&data, 0).unwrap();
        assert_eq!(tag, 0x02);
        assert_eq!(vs, 2);
        assert_eq!(ve, 3);
        assert_eq!(next, 3);
        assert_eq!(data[vs], 5);
    }

    #[test]
    fn decode_tlv_long_form_1_byte() {
        // OCTET STRING, length=200, value = 200 zero bytes.
        // Wire format: tag(1) + 0x81(1) + length_byte(1) + 200 value bytes = 203 bytes total.
        let mut data = [0u8; 203];
        data[0] = 0x04; // OCTET STRING
        data[1] = 0x81; // long form, 1 length byte follows
        data[2] = 200; // length = 200
                       // data[3..203] = 200 zero bytes (value)
        let (tag, vs, ve, next) = decode_tlv(&data, 0).unwrap();
        assert_eq!(tag, 0x04);
        assert_eq!(ve - vs, 200); // 200 value bytes
        assert_eq!(vs, 3); // value starts after tag(1) + 0x81(1) + len(1)
        assert_eq!(ve, 203);
        assert_eq!(next, 203); // past end of the TLV
    }

    #[test]
    fn decode_tlv_truncated_returns_error() {
        let data = [0x02u8, 0x04, 0x00, 0x01]; // INTEGER, length=4, but only 2 bytes of value
        assert_eq!(
            decode_tlv(&data, 0).unwrap_err(),
            DecodeError::LengthOutOfBounds
        );
    }

    #[test]
    fn decode_tlv_empty_returns_unexpected_end() {
        assert_eq!(decode_tlv(&[], 0).unwrap_err(), DecodeError::UnexpectedEnd);
    }

    // ---- GetRequest encode/decode roundtrip --------------------------------

    /// Build a minimal SNMP v2c GetRequest packet for a given OID.
    fn make_get_request(community: &[u8], request_id: i32, oid: &[u32]) -> [u8; 128] {
        let mut buf = [0u8; 128];

        // VarBind content: OID + NULL
        let mut vb_content = [0u8; 64];
        let mut vbp = 0usize;
        vbp = encode_oid(&mut vb_content, vbp, oid).unwrap();
        vbp = encode_null(&mut vb_content, vbp).unwrap();

        // VarBind SEQUENCE
        let mut vb_buf = [0u8; 64];
        let vbh = encode_tlv_header(&mut vb_buf, 0, TAG_SEQUENCE, vbp).unwrap();
        vb_buf[vbh..vbh + vbp].copy_from_slice(&vb_content[..vbp]);
        let vb_total = vbh + vbp;

        // VarBindList SEQUENCE
        let mut vbl_buf = [0u8; 64];
        let vblh = encode_tlv_header(&mut vbl_buf, 0, TAG_SEQUENCE, vb_total).unwrap();
        vbl_buf[vblh..vblh + vb_total].copy_from_slice(&vb_buf[..vb_total]);
        let vbl_total = vblh + vb_total;

        // PDU content: req_id + 0 + 0 + VarBindList
        let mut pdu_content = [0u8; 128];
        let mut pp = 0usize;
        pp = encode_integer(&mut pdu_content, pp, request_id).unwrap();
        pp = encode_integer(&mut pdu_content, pp, 0).unwrap(); // error-status
        pp = encode_integer(&mut pdu_content, pp, 0).unwrap(); // error-index
        pdu_content[pp..pp + vbl_total].copy_from_slice(&vbl_buf[..vbl_total]);
        pp += vbl_total;

        // GetRequest PDU
        let mut pdu_buf = [0u8; 128];
        let pduh = encode_tlv_header(&mut pdu_buf, 0, TAG_GET_REQUEST, pp).unwrap();
        pdu_buf[pduh..pduh + pp].copy_from_slice(&pdu_content[..pp]);
        let pdu_total = pduh + pp;

        // SNMP message content: version + community + PDU
        let mut msg_content = [0u8; 128];
        let mut mp = 0usize;
        mp = encode_integer(&mut msg_content, mp, 1).unwrap(); // v2c
        mp = encode_octet_string(&mut msg_content, mp, community).unwrap();
        msg_content[mp..mp + pdu_total].copy_from_slice(&pdu_buf[..pdu_total]);
        mp += pdu_total;

        // Outer SEQUENCE
        let seqh = encode_tlv_header(&mut buf, 0, TAG_SEQUENCE, mp).unwrap();
        buf[seqh..seqh + mp].copy_from_slice(&msg_content[..mp]);

        buf
    }

    #[test]
    fn decode_get_request_basic() {
        let oid = OID_SYS_UPTIME;
        let buf = make_get_request(b"public", 42, oid);
        let req = decode_get_request(&buf).unwrap();
        assert_eq!(req.version, 1);
        assert_eq!(req.community.as_slice(), b"public");
        assert_eq!(req.request_id, 42);
        assert_eq!(req.pdu_type, TAG_GET_REQUEST);
        assert_eq!(req.oids.len(), 1);
        assert_eq!(req.oids[0].as_slice(), oid);
    }

    #[test]
    fn decode_get_request_wrong_version_v1() {
        // Build a packet with version = 0 (SNMPv1 — we only accept v2c = 1)
        let oid = OID_SYS_UPTIME;
        let mut buf = make_get_request(b"public", 1, oid);
        // The version INTEGER is right after the outer SEQUENCE TLV and is
        // at buf[2] (tag), buf[3] (len), buf[4] (value).
        // Find it: outer SEQUENCE is buf[0] (tag), buf[1] (len if short).
        // Parse to find the version byte position.
        let (_, vs, _, _) = decode_tlv(&buf, 0).unwrap();
        let (_, vvs, _, _) = decode_tlv(&buf[vs..], 0).unwrap();
        // buf[vs + vvs] is the version value byte
        buf[vs + vvs] = 0; // change version to 0 (v1)
        assert_eq!(
            decode_get_request(&buf).unwrap_err(),
            DecodeError::InvalidVersion
        );
    }

    #[test]
    fn decode_get_request_empty_returns_error() {
        assert_eq!(
            decode_get_request(&[]).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
    }

    // ---- GetResponse encode/decode roundtrip -------------------------------

    #[test]
    fn encode_get_response_basic() {
        let mut oid: Vec<u32, 16> = Vec::new();
        for &s in OID_SYS_UPTIME {
            oid.push(s).unwrap();
        }
        let vb = VarBind {
            oid,
            value: SnmpValue::TimeTicks(12345),
        };
        let mut buf = [0u8; 512];
        let n = encode_get_response(&mut buf, 42, b"public", 0, 0, &[vb]).unwrap();
        assert!(n > 0, "response must be non-empty");
        // Outer tag must be SEQUENCE
        assert_eq!(buf[0], TAG_SEQUENCE);
    }

    #[test]
    fn encode_get_response_with_string() {
        let mut oid: Vec<u32, 16> = Vec::new();
        for &s in OID_SYS_DESCR {
            oid.push(s).unwrap();
        }
        let mut val_bytes: Vec<u8, 64> = Vec::new();
        for &b in b"BACnet Bridge v0.1.0" {
            val_bytes.push(b).unwrap();
        }
        let vb = VarBind {
            oid,
            value: SnmpValue::OctetString(val_bytes),
        };
        let mut buf = [0u8; 512];
        let n = encode_get_response(&mut buf, 1, b"public", 0, 0, &[vb]).unwrap();
        assert!(n > 0);
        // Verify "BACnet Bridge" appears in the output
        let output = &buf[..n];
        let found = output.windows(13).any(|w| w == b"BACnet Bridge");
        assert!(found, "device description must be in the response");
    }

    #[test]
    fn encode_get_response_multiple_bindings() {
        let bindings: &[VarBind] = &[
            {
                let mut oid: Vec<u32, 16> = Vec::new();
                for &s in OID_SYS_UPTIME {
                    oid.push(s).unwrap();
                }
                VarBind {
                    oid,
                    value: SnmpValue::TimeTicks(100),
                }
            },
            {
                let mut oid: Vec<u32, 16> = Vec::new();
                for &s in OID_MSTP_FRAMES_SENT {
                    oid.push(s).unwrap();
                }
                VarBind {
                    oid,
                    value: SnmpValue::Counter32(999),
                }
            },
        ];
        let mut buf = [0u8; 512];
        let n = encode_get_response(&mut buf, 7, b"public", 0, 0, bindings).unwrap();
        assert!(n > 0);
    }

    #[test]
    fn encode_get_response_error_status() {
        // noSuchName response with empty bindings
        let mut buf = [0u8; 256];
        let n = encode_get_response(&mut buf, 99, b"public", ERROR_NO_SUCH_NAME, 1, &[]).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], TAG_SEQUENCE);
    }

    // ---- community string check (application level) ------------------------

    #[test]
    fn community_string_is_preserved_in_response() {
        let mut oid: Vec<u32, 16> = Vec::new();
        for &s in OID_SYS_NAME {
            oid.push(s).unwrap();
        }
        let vb = VarBind {
            oid,
            value: SnmpValue::Null,
        };
        let mut buf = [0u8; 256];
        let n = encode_get_response(&mut buf, 1, b"mycommunity", 0, 0, &[vb]).unwrap();
        let output = &buf[..n];
        let found = output.windows(11).any(|w| w == b"mycommunity");
        assert!(found, "community string must appear verbatim in response");
    }

    // ---- OID constant sanity checks ----------------------------------------

    #[test]
    fn oid_constants_have_correct_prefixes() {
        // All System MIB OIDs start with 1.3.6.1.2.1.1
        for oid in [OID_SYS_DESCR, OID_SYS_UPTIME, OID_SYS_CONTACT, OID_SYS_NAME] {
            assert_eq!(
                &oid[..7],
                &[1, 3, 6, 1, 2, 1, 1],
                "System OID prefix wrong: {:?}",
                oid
            );
        }
        // Enterprise OIDs start with 1.3.6.1.4.1.99999
        for oid in [
            OID_MSTP_FRAMES_SENT,
            OID_MSTP_FRAMES_RECV,
            OID_MSTP_TOKEN_LOSSES,
            OID_IPC_DROP_COUNT,
            OID_BACNET_DEVICES_DISCOVERED,
        ] {
            assert_eq!(
                &oid[..7],
                &[1, 3, 6, 1, 4, 1, 99999],
                "Enterprise OID prefix wrong: {:?}",
                oid
            );
        }
    }

    // ---- decode_integer error paths ----------------------------------------

    #[test]
    fn decode_integer_empty_returns_error() {
        assert_eq!(
            decode_integer(&[], 0, 0).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn decode_integer_too_long_returns_error() {
        // 5-byte integer is not supported
        let data = [0x00u8, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            decode_integer(&data, 0, 5).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    // ---- NULL encoding -----------------------------------------------------

    #[test]
    fn encode_null_produces_two_bytes() {
        let mut buf = [0xFFu8; 8];
        let n = encode_null(&mut buf, 0).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], &[TAG_NULL, 0x00]);
    }

    // ---- SnmpValue equality ------------------------------------------------

    #[test]
    fn snmp_value_integer_eq() {
        assert_eq!(SnmpValue::Integer(42), SnmpValue::Integer(42));
        assert_ne!(SnmpValue::Integer(42), SnmpValue::Integer(43));
    }

    #[test]
    fn snmp_value_counter32_eq() {
        assert_eq!(SnmpValue::Counter32(100), SnmpValue::Counter32(100));
        assert_ne!(SnmpValue::Counter32(100), SnmpValue::Counter32(101));
    }

    // ---- Buffer overflow protection ----------------------------------------

    #[test]
    fn encode_integer_buffer_too_small() {
        let mut buf = [0u8; 1]; // needs at least 3 bytes for INTEGER 0
        assert_eq!(
            encode_integer(&mut buf, 0, 0).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    #[test]
    fn encode_oid_buffer_too_small() {
        let mut buf = [0u8; 2]; // far too small for any OID
        assert_eq!(
            encode_oid(&mut buf, 0, OID_SYS_UPTIME).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    #[test]
    fn encode_get_response_buffer_too_small() {
        let mut oid: Vec<u32, 16> = Vec::new();
        for &s in OID_SYS_UPTIME {
            oid.push(s).unwrap();
        }
        let vb = VarBind {
            oid,
            value: SnmpValue::TimeTicks(0),
        };
        let mut buf = [0u8; 8]; // way too small
        assert_eq!(
            encode_get_response(&mut buf, 1, b"public", 0, 0, &[vb]).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }
}
