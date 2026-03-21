//! BACnet APDU (Application Protocol Data Unit) encode/decode.
//!
//! Reference: ASHRAE 135-2020, clause 20–21.
//!
//! Implements encoding and decoding for the most commonly used BACnet
//! services needed by a bridge:
//!
//! - Who-Is / I-Am (device discovery)
//! - ReadProperty / ReadProperty-ACK
//! - WriteProperty
//! - SubscribeCOV / COV-Notification
//!
//! BACnet uses ASN.1-derived tag-length-value encoding. Tags 0–14 fit in
//! one byte; context tags use the tag number from the service definition.

#[cfg(test)]
use crate::bacnet::ObjectType;
use crate::bacnet::{BacnetValue, ObjectId, PropertyId};
use crate::error::{DecodeError, EncodeError};
use heapless::String;

// ---------------------------------------------------------------------------
// ASN.1 tag encoding helpers (ASHRAE 135, clause 20.2)
// ---------------------------------------------------------------------------

/// Encode an application-tagged unsigned integer.
fn encode_app_unsigned(val: u32, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
    // Application tag 2 = Unsigned Integer
    if val <= 0xFF {
        need(buf, *pos, 2)?;
        buf[*pos] = 0x21; // tag 2, length 1
        buf[*pos + 1] = val as u8;
        *pos += 2;
    } else if val <= 0xFFFF {
        need(buf, *pos, 3)?;
        buf[*pos] = 0x22; // tag 2, length 2
        buf[*pos + 1] = (val >> 8) as u8;
        buf[*pos + 2] = val as u8;
        *pos += 3;
    } else if val <= 0xFF_FFFF {
        need(buf, *pos, 4)?;
        buf[*pos] = 0x23; // tag 2, length 3
        buf[*pos + 1] = (val >> 16) as u8;
        buf[*pos + 2] = (val >> 8) as u8;
        buf[*pos + 3] = val as u8;
        *pos += 4;
    } else {
        need(buf, *pos, 5)?;
        buf[*pos] = 0x24; // tag 2, length 4
        buf[*pos + 1] = (val >> 24) as u8;
        buf[*pos + 2] = (val >> 16) as u8;
        buf[*pos + 3] = (val >> 8) as u8;
        buf[*pos + 4] = val as u8;
        *pos += 5;
    }
    Ok(())
}

/// Encode a context-tagged unsigned integer.
fn encode_context_unsigned(
    tag: u8,
    val: u32,
    buf: &mut [u8],
    pos: &mut usize,
) -> Result<(), EncodeError> {
    let tag_hi = (tag & 0x0F) << 4;
    if val <= 0xFF {
        need(buf, *pos, 2)?;
        buf[*pos] = tag_hi | 0x09; // context tag, length 1
        buf[*pos + 1] = val as u8;
        *pos += 2;
    } else if val <= 0xFFFF {
        need(buf, *pos, 3)?;
        buf[*pos] = tag_hi | 0x0A; // context tag, length 2
        buf[*pos + 1] = (val >> 8) as u8;
        buf[*pos + 2] = val as u8;
        *pos += 3;
    } else if val <= 0xFF_FFFF {
        need(buf, *pos, 4)?;
        buf[*pos] = tag_hi | 0x0B; // context tag, length 3
        buf[*pos + 1] = (val >> 16) as u8;
        buf[*pos + 2] = (val >> 8) as u8;
        buf[*pos + 3] = val as u8;
        *pos += 4;
    } else {
        need(buf, *pos, 5)?;
        buf[*pos] = tag_hi | 0x0C; // context tag, length 4
        buf[*pos + 1] = (val >> 24) as u8;
        buf[*pos + 2] = (val >> 16) as u8;
        buf[*pos + 3] = (val >> 8) as u8;
        buf[*pos + 4] = val as u8;
        *pos += 5;
    }
    Ok(())
}

/// Encode an application-tagged object identifier.
fn encode_app_object_id(
    obj: &ObjectId,
    buf: &mut [u8],
    pos: &mut usize,
) -> Result<(), EncodeError> {
    need(buf, *pos, 5)?;
    let raw = obj.to_raw();
    buf[*pos] = 0xC4; // application tag 12, length 4
    buf[*pos + 1] = (raw >> 24) as u8;
    buf[*pos + 2] = (raw >> 16) as u8;
    buf[*pos + 3] = (raw >> 8) as u8;
    buf[*pos + 4] = raw as u8;
    *pos += 5;
    Ok(())
}

/// Encode a context-tagged object identifier.
fn encode_context_object_id(
    tag: u8,
    obj: &ObjectId,
    buf: &mut [u8],
    pos: &mut usize,
) -> Result<(), EncodeError> {
    need(buf, *pos, 5)?;
    let raw = obj.to_raw();
    buf[*pos] = ((tag & 0x0F) << 4) | 0x0C; // context tag, length 4
    buf[*pos + 1] = (raw >> 24) as u8;
    buf[*pos + 2] = (raw >> 16) as u8;
    buf[*pos + 3] = (raw >> 8) as u8;
    buf[*pos + 4] = raw as u8;
    *pos += 5;
    Ok(())
}

/// Encode an application-tagged enumerated value.
fn encode_app_enumerated(val: u32, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
    // Application tag 9 = Enumerated (minimal encoding per clause 20.2.1)
    if val <= 0xFF {
        need(buf, *pos, 2)?;
        buf[*pos] = 0x91; // tag 9, length 1
        buf[*pos + 1] = val as u8;
        *pos += 2;
    } else if val <= 0xFFFF {
        need(buf, *pos, 3)?;
        buf[*pos] = 0x92; // tag 9, length 2
        buf[*pos + 1] = (val >> 8) as u8;
        buf[*pos + 2] = val as u8;
        *pos += 3;
    } else if val <= 0xFF_FFFF {
        need(buf, *pos, 4)?;
        buf[*pos] = 0x93; // tag 9, length 3
        buf[*pos + 1] = (val >> 16) as u8;
        buf[*pos + 2] = (val >> 8) as u8;
        buf[*pos + 3] = val as u8;
        *pos += 4;
    } else {
        need(buf, *pos, 5)?;
        buf[*pos] = 0x94; // tag 9, length 4
        buf[*pos + 1] = (val >> 24) as u8;
        buf[*pos + 2] = (val >> 16) as u8;
        buf[*pos + 3] = (val >> 8) as u8;
        buf[*pos + 4] = val as u8;
        *pos += 5;
    }
    Ok(())
}

/// Encode an opening tag (context constructed, class bit 1, P/C bit 0b110).
fn encode_opening_tag(tag: u8, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
    need(buf, *pos, 1)?;
    buf[*pos] = ((tag & 0x0F) << 4) | 0x0E;
    *pos += 1;
    Ok(())
}

/// Encode a closing tag (context constructed, class bit 1, P/C bit 0b111).
fn encode_closing_tag(tag: u8, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
    need(buf, *pos, 1)?;
    buf[*pos] = ((tag & 0x0F) << 4) | 0x0F;
    *pos += 1;
    Ok(())
}

/// Encode a BacnetValue as application-tagged data.
fn encode_app_value(val: &BacnetValue, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
    match val {
        BacnetValue::Null => {
            need(buf, *pos, 1)?;
            buf[*pos] = 0x00; // tag 0, length 0
            *pos += 1;
        }
        BacnetValue::Boolean(b) => {
            need(buf, *pos, 1)?;
            buf[*pos] = if *b { 0x11 } else { 0x10 }; // tag 1, length encodes value
            *pos += 1;
        }
        BacnetValue::UnsignedInt(v) => {
            encode_app_unsigned(*v, buf, pos)?;
        }
        BacnetValue::SignedInt(v) => {
            // Application tag 3 = Signed Integer
            let bytes = v.to_be_bytes();
            // Find minimum encoding length
            let (start, len) = if *v >= -128 && *v <= 127 {
                (3, 1)
            } else if *v >= -32768 && *v <= 32767 {
                (2, 2)
            } else {
                (0, 4)
            };
            need(buf, *pos, 1 + len)?;
            buf[*pos] = 0x30 | (len as u8); // tag 3, length
            *pos += 1;
            buf[*pos..*pos + len].copy_from_slice(&bytes[start..start + len]);
            *pos += len;
        }
        BacnetValue::Real(v) => {
            need(buf, *pos, 5)?;
            buf[*pos] = 0x44; // tag 4, length 4
            let bytes = v.to_be_bytes();
            buf[*pos + 1..*pos + 5].copy_from_slice(&bytes);
            *pos += 5;
        }
        BacnetValue::CharString(s) => {
            // Application tag 7 = Character String
            // Encoding: tag+length, then 1 byte charset (0=UTF-8), then string bytes
            let slen = s.len() + 1; // +1 for charset byte
            if slen <= 4 {
                need(buf, *pos, 1 + slen)?;
                buf[*pos] = 0x70 | (slen as u8); // tag 7, length
                *pos += 1;
            } else if slen <= 253 {
                need(buf, *pos, 2 + slen)?;
                buf[*pos] = 0x75; // tag 7, extended length
                buf[*pos + 1] = slen as u8;
                *pos += 2;
            } else {
                need(buf, *pos, 4 + slen)?;
                buf[*pos] = 0x75; // tag 7, extended length
                buf[*pos + 1] = 0xFE; // 16-bit length follows
                buf[*pos + 2] = (slen >> 8) as u8;
                buf[*pos + 3] = slen as u8;
                *pos += 4;
            }
            buf[*pos] = 0x00; // UTF-8 charset
            *pos += 1;
            buf[*pos..*pos + s.len()].copy_from_slice(s.as_bytes());
            *pos += s.len();
        }
        BacnetValue::Enumerated(v) => {
            encode_app_enumerated(*v, buf, pos)?;
        }
        BacnetValue::ObjectIdentifier(obj) => {
            encode_app_object_id(obj, buf, pos)?;
        }
    }
    Ok(())
}

/// Check that `buf` has at least `n` more bytes from position `pos`.
#[inline]
fn need(buf: &[u8], pos: usize, n: usize) -> Result<(), EncodeError> {
    if pos + n > buf.len() {
        Err(EncodeError::BufferTooSmall)
    } else {
        Ok(())
    }
}

/// Check that `data` has at least `n` more bytes from position `pos`.
#[inline]
fn dneed(data: &[u8], pos: usize, n: usize) -> Result<(), DecodeError> {
    if pos + n > data.len() {
        Err(DecodeError::UnexpectedEnd)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ASN.1 tag decoding helpers
// ---------------------------------------------------------------------------

/// Decoded tag info.
struct TagInfo {
    /// Tag number (0–14 for application, context-specific otherwise).
    number: u8,
    /// True if this is a context-specific tag (class bit set).
    context: bool,
    /// Length/value (LVT) field. For opening/closing tags this is 6 or 7.
    lvt: u32,
    /// Total bytes consumed (tag + extended tag/length bytes).
    header_len: usize,
}

/// Decode a BACnet ASN.1 tag at the given position.
fn decode_tag(data: &[u8], pos: usize) -> Result<TagInfo, DecodeError> {
    dneed(data, pos, 1)?;
    let b = data[pos];
    let mut hlen = 1usize;

    // Tag number: bits 7–4
    let mut number = (b >> 4) & 0x0F;
    if number == 0x0F {
        // Extended tag number
        dneed(data, pos, 2)?;
        number = data[pos + 1];
        hlen = 2;
    }

    let context = (b & 0x08) != 0;
    let lvt_raw = b & 0x07;

    // Opening tag (0x0E & 0x07 = 6) and closing tag (0x0F & 0x07 = 7)
    if context && (lvt_raw == 6 || lvt_raw == 7) {
        return Ok(TagInfo {
            number,
            context,
            lvt: lvt_raw as u32,
            header_len: hlen,
        });
    }

    // LVT field
    let lvt = if lvt_raw == 5 {
        // Extended length
        dneed(data, pos, hlen + 1)?;
        let ext = data[pos + hlen];
        hlen += 1;
        if ext < 254 {
            ext as u32
        } else if ext == 254 {
            dneed(data, pos, hlen + 2)?;
            let v = ((data[pos + hlen] as u32) << 8) | (data[pos + hlen + 1] as u32);
            hlen += 2;
            v
        } else {
            dneed(data, pos, hlen + 4)?;
            let v = ((data[pos + hlen] as u32) << 24)
                | ((data[pos + hlen + 1] as u32) << 16)
                | ((data[pos + hlen + 2] as u32) << 8)
                | (data[pos + hlen + 3] as u32);
            hlen += 4;
            v
        }
    } else {
        lvt_raw as u32
    };

    Ok(TagInfo {
        number,
        context,
        lvt,
        header_len: hlen,
    })
}

/// Decode an unsigned integer from `data[pos..pos+len]`.
fn decode_unsigned(data: &[u8], pos: usize, len: u32) -> Result<u32, DecodeError> {
    let len = len as usize;
    dneed(data, pos, len)?;
    let mut val = 0u32;
    for i in 0..len {
        val = (val << 8) | (data[pos + i] as u32);
    }
    Ok(val)
}

/// Decode a signed integer from `data[pos..pos+len]`.
fn decode_signed(data: &[u8], pos: usize, len: u32) -> Result<i32, DecodeError> {
    let len = len as usize;
    dneed(data, pos, len)?;
    // Sign-extend from the first byte
    let mut val = if data[pos] & 0x80 != 0 { -1i32 } else { 0i32 };
    for i in 0..len {
        val = (val << 8) | (data[pos + i] as i32);
    }
    Ok(val)
}

/// Decode an application-tagged BacnetValue.
fn decode_app_value(data: &[u8], pos: &mut usize) -> Result<BacnetValue, DecodeError> {
    let tag = decode_tag(data, *pos)?;
    if tag.context {
        return Err(DecodeError::InvalidData);
    }
    let vpos = *pos + tag.header_len;
    let vlen = tag.lvt;

    let val = match tag.number {
        0 => BacnetValue::Null, // tag 0 = Null
        1 => {
            // Boolean: value is in the LVT field, no data bytes follow.
            let v = BacnetValue::Boolean(tag.lvt != 0);
            *pos = vpos; // no data bytes to skip
            return Ok(v);
        }
        2 => BacnetValue::UnsignedInt(decode_unsigned(data, vpos, vlen)?),
        3 => BacnetValue::SignedInt(decode_signed(data, vpos, vlen)?),
        4 => {
            // Real (IEEE 754 float)
            if vlen != 4 {
                return Err(DecodeError::InvalidData);
            }
            dneed(data, vpos, 4)?;
            let bits = ((data[vpos] as u32) << 24)
                | ((data[vpos + 1] as u32) << 16)
                | ((data[vpos + 2] as u32) << 8)
                | (data[vpos + 3] as u32);
            BacnetValue::Real(f32::from_bits(bits))
        }
        7 => {
            // Character String: first byte is charset (0=UTF-8), rest is string
            if vlen < 1 {
                return Err(DecodeError::InvalidData);
            }
            dneed(data, vpos, vlen as usize)?;
            let _charset = data[vpos]; // 0 = UTF-8
            let slen = (vlen - 1) as usize;
            let sbytes = &data[vpos + 1..vpos + 1 + slen];
            let s = core::str::from_utf8(sbytes).map_err(|_| DecodeError::InvalidData)?;
            let mut hs: String<64> = String::new();
            if hs.push_str(s).is_err() {
                // Truncate at a safe UTF-8 char boundary
                let mut end = 64.min(s.len());
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                let _ = hs.push_str(&s[..end]);
            }
            BacnetValue::CharString(hs)
        }
        9 => BacnetValue::Enumerated(decode_unsigned(data, vpos, vlen)?),
        12 => {
            // Object Identifier
            if vlen != 4 {
                return Err(DecodeError::InvalidData);
            }
            let raw = decode_unsigned(data, vpos, 4)?;
            let obj = ObjectId::from_raw(raw).ok_or(DecodeError::InvalidData)?;
            BacnetValue::ObjectIdentifier(obj)
        }
        _ => {
            // Skip unknown application tags
            *pos = vpos + vlen as usize;
            return Ok(BacnetValue::Null);
        }
    };

    *pos = vpos + vlen as usize;
    Ok(val)
}

/// Decode a context-tagged unsigned, returning None if tag doesn't match.
fn decode_context_unsigned(
    data: &[u8],
    pos: &mut usize,
    expected_tag: u8,
) -> Result<Option<u32>, DecodeError> {
    if *pos >= data.len() {
        return Ok(None);
    }
    let tag = decode_tag(data, *pos)?;
    if !tag.context || tag.number != expected_tag {
        return Ok(None);
    }
    let vpos = *pos + tag.header_len;
    let val = decode_unsigned(data, vpos, tag.lvt)?;
    *pos = vpos + tag.lvt as usize;
    Ok(Some(val))
}

/// Decode a context-tagged object identifier.
fn decode_context_object_id(
    data: &[u8],
    pos: &mut usize,
    expected_tag: u8,
) -> Result<Option<ObjectId>, DecodeError> {
    if *pos >= data.len() {
        return Ok(None);
    }
    let tag = decode_tag(data, *pos)?;
    if !tag.context || tag.number != expected_tag || tag.lvt != 4 {
        return Ok(None);
    }
    let vpos = *pos + tag.header_len;
    let raw = decode_unsigned(data, vpos, 4)?;
    let obj = ObjectId::from_raw(raw).ok_or(DecodeError::InvalidData)?;
    *pos = vpos + 4;
    Ok(Some(obj))
}

/// Check for and skip an opening tag.
fn expect_opening_tag(data: &[u8], pos: &mut usize, expected_tag: u8) -> Result<(), DecodeError> {
    let tag = decode_tag(data, *pos)?;
    if !tag.context || tag.number != expected_tag || tag.lvt != 6 {
        return Err(DecodeError::InvalidData);
    }
    *pos += tag.header_len;
    Ok(())
}

/// Check for and skip a closing tag.
fn expect_closing_tag(data: &[u8], pos: &mut usize, expected_tag: u8) -> Result<(), DecodeError> {
    let tag = decode_tag(data, *pos)?;
    if !tag.context || tag.number != expected_tag || tag.lvt != 7 {
        return Err(DecodeError::InvalidData);
    }
    *pos += tag.header_len;
    Ok(())
}

/// Check if the byte at `pos` is a closing tag for the given tag number.
fn is_closing_tag(data: &[u8], pos: usize, tag_num: u8) -> bool {
    if pos >= data.len() {
        return false;
    }
    let b = data[pos];
    let number = (b >> 4) & 0x0F;
    let is_context = (b & 0x08) != 0;
    let lvt = b & 0x07;
    is_context && number == tag_num && lvt == 7
}

// ===========================================================================
// APDU type bytes
// ===========================================================================

/// PDU type for Unconfirmed-Request (high nibble 0x1).
pub const PDU_UNCONFIRMED_REQUEST: u8 = 0x10;
/// PDU type for Confirmed-Request (high nibble 0x0).
pub const PDU_CONFIRMED_REQUEST: u8 = 0x00;
/// PDU type for Simple-ACK (high nibble 0x2).
pub const PDU_SIMPLE_ACK: u8 = 0x20;
/// PDU type for Complex-ACK (high nibble 0x3).
pub const PDU_COMPLEX_ACK: u8 = 0x30;
/// PDU type for Error (high nibble 0x5).
pub const PDU_ERROR: u8 = 0x50;

// ===========================================================================
// Service codes
// ===========================================================================

/// Unconfirmed service: Who-Is.
pub const SERVICE_WHO_IS: u8 = 0x08;
/// Unconfirmed service: I-Am.
pub const SERVICE_I_AM: u8 = 0x00;
/// Unconfirmed service: Unconfirmed-COV-Notification.
pub const SERVICE_UCOV_NOTIFICATION: u8 = 0x02;
/// Confirmed service: SubscribeCOV.
pub const SERVICE_SUBSCRIBE_COV: u8 = 0x05;
/// Confirmed service: ReadProperty.
pub const SERVICE_READ_PROPERTY: u8 = 0x0C;
/// Confirmed service: ReadPropertyMultiple.
pub const SERVICE_READ_PROPERTY_MULTIPLE: u8 = 0x0E;
/// Confirmed service: WriteProperty.
pub const SERVICE_WRITE_PROPERTY: u8 = 0x0F;

// ===========================================================================
// Parsed service structs
// ===========================================================================

/// Parsed Who-Is request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhoIsRequest {
    /// Lower device instance limit (inclusive), or None for unbounded.
    pub low_limit: Option<u32>,
    /// Upper device instance limit (inclusive), or None for unbounded.
    pub high_limit: Option<u32>,
}

/// Data for encoding an I-Am response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IAmData {
    /// Our device object identifier.
    pub device_id: ObjectId,
    /// Maximum APDU length we accept.
    pub max_apdu: u32,
    /// Segmentation supported: 0=both, 1=transmit, 2=receive, 3=none.
    pub segmentation: u8,
    /// Vendor identifier.
    pub vendor_id: u16,
}

/// Parsed ReadProperty request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPropertyRequest {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub array_index: Option<u32>,
}

/// Data for encoding a ReadProperty-ACK.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPropertyAck {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub array_index: Option<u32>,
    pub value: BacnetValue,
}

/// Parsed WriteProperty request.
#[derive(Debug, Clone, PartialEq)]
pub struct WritePropertyRequest {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub array_index: Option<u32>,
    pub value: BacnetValue,
    pub priority: Option<u8>,
}

/// Parsed SubscribeCOV request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscribeCovRequest {
    pub subscriber_process_id: u32,
    pub monitored_object: ObjectId,
    /// If true, send Confirmed-COV-Notification; if false, Unconfirmed.
    pub issue_confirmed: bool,
    /// Subscription lifetime in seconds (None = indefinite).
    pub lifetime: Option<u32>,
}

/// Unconfirmed COV notification data.
#[derive(Debug, Clone, PartialEq)]
pub struct CovNotification {
    pub subscriber_process_id: u32,
    pub initiating_device: ObjectId,
    pub monitored_object: ObjectId,
    pub time_remaining: u32,
    /// List of (property_id, value) pairs.
    pub values: heapless::Vec<(PropertyId, BacnetValue), 8>,
}

/// Decoded APDU — the top-level parsed result.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum DecodedApdu {
    WhoIs(WhoIsRequest),
    IAm(IAmData),
    ReadProperty(ReadPropertyRequest, u8), // request + invoke_id
    ReadPropertyAck(ReadPropertyAck, u8),  // ack + invoke_id
    WriteProperty(WritePropertyRequest, u8), // request + invoke_id
    SubscribeCov(SubscribeCovRequest, u8), // request + invoke_id
    UnconfirmedCovNotification(CovNotification),
    SimpleAck(u8, u8),       // invoke_id, service
    Error(u8, u8, u16, u16), // invoke_id, service, error_class, error_code
    /// Unknown or unsupported — raw PDU type and service code.
    Unknown(u8, u8),
}

// ===========================================================================
// Encoding functions
// ===========================================================================

/// Encode a Who-Is unconfirmed request.
///
/// If both limits are None, encodes an unbounded Who-Is (2 bytes).
/// If both limits are Some, encodes with device instance range.
pub fn encode_who_is(
    low: Option<u32>,
    high: Option<u32>,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 2)?;
    buf[pos] = PDU_UNCONFIRMED_REQUEST;
    buf[pos + 1] = SERVICE_WHO_IS;
    pos += 2;

    if let (Some(lo), Some(hi)) = (low, high) {
        encode_context_unsigned(0, lo, buf, &mut pos)?;
        encode_context_unsigned(1, hi, buf, &mut pos)?;
    }

    Ok(pos)
}

/// Encode an I-Am unconfirmed response.
pub fn encode_i_am(iam: &IAmData, buf: &mut [u8]) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 2)?;
    buf[pos] = PDU_UNCONFIRMED_REQUEST;
    buf[pos + 1] = SERVICE_I_AM;
    pos += 2;

    encode_app_object_id(&iam.device_id, buf, &mut pos)?;
    encode_app_unsigned(iam.max_apdu, buf, &mut pos)?;
    encode_app_enumerated(iam.segmentation as u32, buf, &mut pos)?;
    // Vendor ID as unsigned (application tag 2)
    encode_app_unsigned(iam.vendor_id as u32, buf, &mut pos)?;

    Ok(pos)
}

/// Encode a ReadProperty confirmed request.
pub fn encode_read_property(
    req: &ReadPropertyRequest,
    invoke_id: u8,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 4)?;
    buf[pos] = PDU_CONFIRMED_REQUEST;
    buf[pos + 1] = 0x05; // max segments=0, max APDU=1476
    buf[pos + 2] = invoke_id;
    buf[pos + 3] = SERVICE_READ_PROPERTY;
    pos += 4;

    encode_context_object_id(0, &req.object_id, buf, &mut pos)?;
    encode_context_unsigned(1, req.property_id.code(), buf, &mut pos)?;
    if let Some(idx) = req.array_index {
        encode_context_unsigned(2, idx, buf, &mut pos)?;
    }

    Ok(pos)
}

/// Encode a ReadProperty-ACK complex response.
pub fn encode_read_property_ack(
    ack: &ReadPropertyAck,
    invoke_id: u8,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 3)?;
    buf[pos] = PDU_COMPLEX_ACK;
    buf[pos + 1] = invoke_id;
    buf[pos + 2] = SERVICE_READ_PROPERTY;
    pos += 3;

    encode_context_object_id(0, &ack.object_id, buf, &mut pos)?;
    encode_context_unsigned(1, ack.property_id.code(), buf, &mut pos)?;
    if let Some(idx) = ack.array_index {
        encode_context_unsigned(2, idx, buf, &mut pos)?;
    }

    // Property value: opening tag 3, value, closing tag 3
    encode_opening_tag(3, buf, &mut pos)?;
    encode_app_value(&ack.value, buf, &mut pos)?;
    encode_closing_tag(3, buf, &mut pos)?;

    Ok(pos)
}

/// Encode a WriteProperty confirmed request.
pub fn encode_write_property(
    req: &WritePropertyRequest,
    invoke_id: u8,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 4)?;
    buf[pos] = PDU_CONFIRMED_REQUEST;
    buf[pos + 1] = 0x05; // max segments=0, max APDU=1476
    buf[pos + 2] = invoke_id;
    buf[pos + 3] = SERVICE_WRITE_PROPERTY;
    pos += 4;

    encode_context_object_id(0, &req.object_id, buf, &mut pos)?;
    encode_context_unsigned(1, req.property_id.code(), buf, &mut pos)?;
    if let Some(idx) = req.array_index {
        encode_context_unsigned(2, idx, buf, &mut pos)?;
    }

    // Property value: opening tag 3, value, closing tag 3
    encode_opening_tag(3, buf, &mut pos)?;
    encode_app_value(&req.value, buf, &mut pos)?;
    encode_closing_tag(3, buf, &mut pos)?;

    if let Some(p) = req.priority {
        encode_context_unsigned(4, p as u32, buf, &mut pos)?;
    }

    Ok(pos)
}

/// Encode a Simple-ACK response.
pub fn encode_simple_ack(invoke_id: u8, service: u8, buf: &mut [u8]) -> Result<usize, EncodeError> {
    need(buf, 0, 3)?;
    buf[0] = PDU_SIMPLE_ACK;
    buf[1] = invoke_id;
    buf[2] = service;
    Ok(3)
}

/// Encode a BACnet Error response.
pub fn encode_error(
    invoke_id: u8,
    service: u8,
    error_class: u16,
    error_code: u16,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 3)?;
    buf[pos] = PDU_ERROR;
    buf[pos + 1] = invoke_id;
    buf[pos + 2] = service;
    pos += 3;

    encode_app_enumerated(error_class as u32, buf, &mut pos)?;
    encode_app_enumerated(error_code as u32, buf, &mut pos)?;

    Ok(pos)
}

/// Encode a SubscribeCOV confirmed request.
pub fn encode_subscribe_cov(
    req: &SubscribeCovRequest,
    invoke_id: u8,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 4)?;
    buf[pos] = PDU_CONFIRMED_REQUEST;
    buf[pos + 1] = 0x05;
    buf[pos + 2] = invoke_id;
    buf[pos + 3] = SERVICE_SUBSCRIBE_COV;
    pos += 4;

    encode_context_unsigned(0, req.subscriber_process_id, buf, &mut pos)?;
    encode_context_object_id(1, &req.monitored_object, buf, &mut pos)?;

    // Context tag 2: issue confirmed notifications (boolean as unsigned 0/1)
    encode_context_unsigned(2, if req.issue_confirmed { 1 } else { 0 }, buf, &mut pos)?;

    if let Some(lt) = req.lifetime {
        encode_context_unsigned(3, lt, buf, &mut pos)?;
    }

    Ok(pos)
}

/// Encode an Unconfirmed-COV-Notification.
pub fn encode_ucov_notification(
    notif: &CovNotification,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    need(buf, pos, 2)?;
    buf[pos] = PDU_UNCONFIRMED_REQUEST;
    buf[pos + 1] = SERVICE_UCOV_NOTIFICATION;
    pos += 2;

    encode_context_unsigned(0, notif.subscriber_process_id, buf, &mut pos)?;
    encode_context_object_id(1, &notif.initiating_device, buf, &mut pos)?;
    encode_context_object_id(2, &notif.monitored_object, buf, &mut pos)?;
    encode_context_unsigned(3, notif.time_remaining, buf, &mut pos)?;

    // Tag 4: list of values (opening/closing)
    encode_opening_tag(4, buf, &mut pos)?;
    for (prop, val) in &notif.values {
        // Each entry: [0] property-identifier, [1] (currently unused), [2] value
        encode_context_unsigned(0, prop.code(), buf, &mut pos)?;
        // property-array-index [1] — omitted (optional)
        encode_opening_tag(2, buf, &mut pos)?;
        encode_app_value(val, buf, &mut pos)?;
        encode_closing_tag(2, buf, &mut pos)?;
    }
    encode_closing_tag(4, buf, &mut pos)?;

    Ok(pos)
}

// ===========================================================================
// Decoding functions
// ===========================================================================

/// Decode an APDU from raw bytes.
///
/// Returns a `DecodedApdu` variant matching the PDU type and service.
pub fn decode_apdu(data: &[u8]) -> Result<DecodedApdu, DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::UnexpectedEnd);
    }

    let pdu_type = data[0] & 0xF0;

    match pdu_type {
        0x10 => {
            // Unconfirmed Request
            dneed(data, 0, 2)?;
            let service = data[1];
            match service {
                SERVICE_WHO_IS => decode_who_is(&data[2..]),
                SERVICE_I_AM => decode_i_am(&data[2..]),
                SERVICE_UCOV_NOTIFICATION => decode_ucov_notification(&data[2..]),
                _ => Ok(DecodedApdu::Unknown(pdu_type, service)),
            }
        }
        0x00 => {
            // Confirmed Request
            dneed(data, 0, 4)?;
            let invoke_id = data[2];
            let service = data[3];
            match service {
                SERVICE_READ_PROPERTY => decode_read_property(&data[4..], invoke_id),
                SERVICE_WRITE_PROPERTY => decode_write_property(&data[4..], invoke_id),
                SERVICE_SUBSCRIBE_COV => decode_subscribe_cov(&data[4..], invoke_id),
                _ => Ok(DecodedApdu::Unknown(pdu_type, service)),
            }
        }
        0x30 => {
            // Complex-ACK
            dneed(data, 0, 3)?;
            let invoke_id = data[1];
            let service = data[2];
            match service {
                SERVICE_READ_PROPERTY => decode_read_property_ack(&data[3..], invoke_id),
                _ => Ok(DecodedApdu::Unknown(pdu_type, service)),
            }
        }
        0x20 => {
            // Simple-ACK
            dneed(data, 0, 3)?;
            Ok(DecodedApdu::SimpleAck(data[1], data[2]))
        }
        0x50 => {
            // Error
            dneed(data, 0, 3)?;
            let invoke_id = data[1];
            let service = data[2];
            let mut pos = 3;
            let error_class = if pos < data.len() {
                let tag = decode_tag(data, pos)?;
                let v = decode_unsigned(data, pos + tag.header_len, tag.lvt)?;
                pos = pos + tag.header_len + tag.lvt as usize;
                v as u16
            } else {
                0
            };
            let error_code = if pos < data.len() {
                let tag = decode_tag(data, pos)?;
                let v = decode_unsigned(data, pos + tag.header_len, tag.lvt)?;
                v as u16
            } else {
                0
            };
            Ok(DecodedApdu::Error(
                invoke_id,
                service,
                error_class,
                error_code,
            ))
        }
        _ => Ok(DecodedApdu::Unknown(
            pdu_type,
            data.get(1).copied().unwrap_or(0),
        )),
    }
}

fn decode_who_is(data: &[u8]) -> Result<DecodedApdu, DecodeError> {
    if data.is_empty() {
        return Ok(DecodedApdu::WhoIs(WhoIsRequest {
            low_limit: None,
            high_limit: None,
        }));
    }

    let mut pos = 0;
    let low = decode_context_unsigned(data, &mut pos, 0)?;
    let high = decode_context_unsigned(data, &mut pos, 1)?;

    Ok(DecodedApdu::WhoIs(WhoIsRequest {
        low_limit: low,
        high_limit: high,
    }))
}

fn decode_i_am(data: &[u8]) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    // I-Am object identifier (application tag 12)
    let device_val = decode_app_value(data, &mut pos)?;
    let device_id = match device_val {
        BacnetValue::ObjectIdentifier(id) => id,
        _ => return Err(DecodeError::InvalidData),
    };

    // Max APDU length accepted (application tag 2)
    let max_apdu_val = decode_app_value(data, &mut pos)?;
    let max_apdu = match max_apdu_val {
        BacnetValue::UnsignedInt(v) => v,
        _ => return Err(DecodeError::InvalidData),
    };

    // Segmentation supported (application tag 9)
    let seg_val = decode_app_value(data, &mut pos)?;
    let segmentation = match seg_val {
        BacnetValue::Enumerated(v) => v as u8,
        _ => return Err(DecodeError::InvalidData),
    };

    // Vendor ID (application tag 2)
    let vendor_val = decode_app_value(data, &mut pos)?;
    let vendor_id = match vendor_val {
        BacnetValue::UnsignedInt(v) => v as u16,
        _ => return Err(DecodeError::InvalidData),
    };

    Ok(DecodedApdu::IAm(IAmData {
        device_id,
        max_apdu,
        segmentation,
        vendor_id,
    }))
}

fn decode_read_property(data: &[u8], invoke_id: u8) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    let object_id = decode_context_object_id(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
    let prop_code = decode_context_unsigned(data, &mut pos, 1)?.ok_or(DecodeError::InvalidData)?;
    let array_index = decode_context_unsigned(data, &mut pos, 2)?;

    Ok(DecodedApdu::ReadProperty(
        ReadPropertyRequest {
            object_id,
            property_id: PropertyId::from_code(prop_code),
            array_index,
        },
        invoke_id,
    ))
}

fn decode_read_property_ack(data: &[u8], invoke_id: u8) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    let object_id = decode_context_object_id(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
    let prop_code = decode_context_unsigned(data, &mut pos, 1)?.ok_or(DecodeError::InvalidData)?;
    let array_index = decode_context_unsigned(data, &mut pos, 2)?;

    // Opening tag 3
    expect_opening_tag(data, &mut pos, 3)?;
    let value = decode_app_value(data, &mut pos)?;
    expect_closing_tag(data, &mut pos, 3)?;

    Ok(DecodedApdu::ReadPropertyAck(
        ReadPropertyAck {
            object_id,
            property_id: PropertyId::from_code(prop_code),
            array_index,
            value,
        },
        invoke_id,
    ))
}

fn decode_write_property(data: &[u8], invoke_id: u8) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    let object_id = decode_context_object_id(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
    let prop_code = decode_context_unsigned(data, &mut pos, 1)?.ok_or(DecodeError::InvalidData)?;
    let array_index = decode_context_unsigned(data, &mut pos, 2)?;

    // Opening tag 3 — property value
    expect_opening_tag(data, &mut pos, 3)?;
    let value = decode_app_value(data, &mut pos)?;
    expect_closing_tag(data, &mut pos, 3)?;

    // Optional priority (context tag 4)
    let priority = decode_context_unsigned(data, &mut pos, 4)?.map(|v| v as u8);

    Ok(DecodedApdu::WriteProperty(
        WritePropertyRequest {
            object_id,
            property_id: PropertyId::from_code(prop_code),
            array_index,
            value,
            priority,
        },
        invoke_id,
    ))
}

fn decode_subscribe_cov(data: &[u8], invoke_id: u8) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    let subscriber_process_id =
        decode_context_unsigned(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
    let monitored_object =
        decode_context_object_id(data, &mut pos, 1)?.ok_or(DecodeError::InvalidData)?;

    // Optional: issue confirmed (context tag 2, boolean-as-unsigned)
    let issue_confirmed = decode_context_unsigned(data, &mut pos, 2)?
        .map(|v| v != 0)
        .unwrap_or(false);

    // Optional: lifetime (context tag 3)
    let lifetime = decode_context_unsigned(data, &mut pos, 3)?;

    Ok(DecodedApdu::SubscribeCov(
        SubscribeCovRequest {
            subscriber_process_id,
            monitored_object,
            issue_confirmed,
            lifetime,
        },
        invoke_id,
    ))
}

fn decode_ucov_notification(data: &[u8]) -> Result<DecodedApdu, DecodeError> {
    let mut pos = 0;

    let subscriber_process_id =
        decode_context_unsigned(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
    let initiating_device =
        decode_context_object_id(data, &mut pos, 1)?.ok_or(DecodeError::InvalidData)?;
    let monitored_object =
        decode_context_object_id(data, &mut pos, 2)?.ok_or(DecodeError::InvalidData)?;
    let time_remaining =
        decode_context_unsigned(data, &mut pos, 3)?.ok_or(DecodeError::InvalidData)?;

    let mut values: heapless::Vec<(PropertyId, BacnetValue), 8> = heapless::Vec::new();

    // Opening tag 4: list of values
    if pos < data.len() {
        expect_opening_tag(data, &mut pos, 4)?;
        while !is_closing_tag(data, pos, 4) {
            // Each entry: [0] property-identifier, optional [1] array-index, [2] { value }
            let prop_code =
                decode_context_unsigned(data, &mut pos, 0)?.ok_or(DecodeError::InvalidData)?;
            // Skip optional array-index (tag 1)
            let _ = decode_context_unsigned(data, &mut pos, 1)?;
            // Opening tag 2: value
            expect_opening_tag(data, &mut pos, 2)?;
            let val = decode_app_value(data, &mut pos)?;
            expect_closing_tag(data, &mut pos, 2)?;

            let _ = values.push((PropertyId::from_code(prop_code), val));
        }
        expect_closing_tag(data, &mut pos, 4)?;
    }

    Ok(DecodedApdu::UnconfirmedCovNotification(CovNotification {
        subscriber_process_id,
        initiating_device,
        monitored_object,
        time_remaining,
        values,
    }))
}

// ===========================================================================
// Unit tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Who-Is ----

    #[test]
    fn who_is_unbounded_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_who_is(None, None, &mut buf).unwrap();
        assert_eq!(n, 2);
        assert_eq!(buf[0], PDU_UNCONFIRMED_REQUEST);
        assert_eq!(buf[1], SERVICE_WHO_IS);

        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(
            decoded,
            DecodedApdu::WhoIs(WhoIsRequest {
                low_limit: None,
                high_limit: None,
            })
        );
    }

    #[test]
    fn who_is_bounded_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_who_is(Some(100), Some(200), &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(
            decoded,
            DecodedApdu::WhoIs(WhoIsRequest {
                low_limit: Some(100),
                high_limit: Some(200),
            })
        );
    }

    #[test]
    fn who_is_large_limits() {
        let mut buf = [0u8; 64];
        let n = encode_who_is(Some(0), Some(4194302), &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        match decoded {
            DecodedApdu::WhoIs(req) => {
                assert_eq!(req.low_limit, Some(0));
                assert_eq!(req.high_limit, Some(4194302));
            }
            _ => panic!("expected WhoIs"),
        }
    }

    // ---- I-Am ----

    #[test]
    fn i_am_round_trip() {
        let iam = IAmData {
            device_id: ObjectId::new(ObjectType::Device, 1234),
            max_apdu: 1476,
            segmentation: 3, // no segmentation
            vendor_id: 0xFFFF,
        };
        let mut buf = [0u8; 64];
        let n = encode_i_am(&iam, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::IAm(iam));
    }

    // ---- ReadProperty ----

    #[test]
    fn read_property_round_trip() {
        let req = ReadPropertyRequest {
            object_id: ObjectId::new(ObjectType::AnalogInput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property(&req, 42, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadProperty(req, 42));
    }

    #[test]
    fn read_property_with_index() {
        let req = ReadPropertyRequest {
            object_id: ObjectId::new(ObjectType::MultiStateValue, 5),
            property_id: PropertyId::Raw(87), // PriorityArray
            array_index: Some(3),
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property(&req, 7, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadProperty(req, 7));
    }

    // ---- ReadProperty-ACK ----

    #[test]
    fn read_property_ack_real() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::AnalogInput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Real(23.5),
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property_ack(&ack, 42, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 42));
    }

    #[test]
    fn read_property_ack_string() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::Device, 100),
            property_id: PropertyId::ObjectName,
            array_index: None,
            value: BacnetValue::CharString(String::try_from("TestDevice").unwrap()),
        };
        let mut buf = [0u8; 128];
        let n = encode_read_property_ack(&ack, 1, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 1));
    }

    #[test]
    fn read_property_ack_boolean() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::BinaryInput, 0),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Boolean(true),
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property_ack(&ack, 5, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 5));
    }

    #[test]
    fn read_property_ack_unsigned() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::AnalogValue, 10),
            property_id: PropertyId::Units,
            array_index: None,
            value: BacnetValue::UnsignedInt(62), // degreesCelsius
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property_ack(&ack, 3, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 3));
    }

    #[test]
    fn read_property_ack_enumerated() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::MultiStateInput, 0),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Enumerated(2),
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property_ack(&ack, 9, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 9));
    }

    // ---- WriteProperty ----

    #[test]
    fn write_property_round_trip() {
        let req = WritePropertyRequest {
            object_id: ObjectId::new(ObjectType::AnalogOutput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Real(72.0),
            priority: Some(8),
        };
        let mut buf = [0u8; 64];
        let n = encode_write_property(&req, 10, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::WriteProperty(req, 10));
    }

    #[test]
    fn write_property_no_priority() {
        let req = WritePropertyRequest {
            object_id: ObjectId::new(ObjectType::BinaryOutput, 5),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Enumerated(1), // active
            priority: None,
        };
        let mut buf = [0u8; 64];
        let n = encode_write_property(&req, 3, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::WriteProperty(req, 3));
    }

    // ---- SubscribeCOV ----

    #[test]
    fn subscribe_cov_round_trip() {
        let req = SubscribeCovRequest {
            subscriber_process_id: 1,
            monitored_object: ObjectId::new(ObjectType::AnalogInput, 0),
            issue_confirmed: false,
            lifetime: Some(300),
        };
        let mut buf = [0u8; 64];
        let n = encode_subscribe_cov(&req, 15, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::SubscribeCov(req, 15));
    }

    #[test]
    fn subscribe_cov_no_lifetime() {
        let req = SubscribeCovRequest {
            subscriber_process_id: 42,
            monitored_object: ObjectId::new(ObjectType::BinaryValue, 10),
            issue_confirmed: true,
            lifetime: None,
        };
        let mut buf = [0u8; 64];
        let n = encode_subscribe_cov(&req, 20, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::SubscribeCov(req, 20));
    }

    // ---- COV Notification ----

    #[test]
    fn ucov_notification_round_trip() {
        let mut values: heapless::Vec<(PropertyId, BacnetValue), 8> = heapless::Vec::new();
        values
            .push((PropertyId::PresentValue, BacnetValue::Real(21.5)))
            .unwrap();
        values
            .push((
                PropertyId::StatusFlags,
                BacnetValue::UnsignedInt(0), // all flags clear
            ))
            .unwrap();

        let notif = CovNotification {
            subscriber_process_id: 1,
            initiating_device: ObjectId::new(ObjectType::Device, 100),
            monitored_object: ObjectId::new(ObjectType::AnalogInput, 0),
            time_remaining: 60,
            values,
        };
        let mut buf = [0u8; 128];
        let n = encode_ucov_notification(&notif, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::UnconfirmedCovNotification(notif));
    }

    // ---- Simple-ACK ----

    #[test]
    fn simple_ack_round_trip() {
        let mut buf = [0u8; 8];
        let n = encode_simple_ack(5, SERVICE_WRITE_PROPERTY, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::SimpleAck(5, SERVICE_WRITE_PROPERTY));
    }

    // ---- Error ----

    #[test]
    fn error_round_trip() {
        let mut buf = [0u8; 16];
        let n = encode_error(5, SERVICE_READ_PROPERTY, 2, 31, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::Error(5, SERVICE_READ_PROPERTY, 2, 31));
    }

    // ---- Known wire captures ----

    #[test]
    fn decode_real_who_is_broadcast() {
        // Captured from a real BACnet controller
        let apdu: &[u8] = &[0x10, 0x08]; // Unconfirmed, Who-Is, no limits
        let decoded = decode_apdu(apdu).unwrap();
        assert_eq!(
            decoded,
            DecodedApdu::WhoIs(WhoIsRequest {
                low_limit: None,
                high_limit: None,
            })
        );
    }

    #[test]
    fn decode_real_i_am() {
        // I-Am from device 1234, max APDU 480, no segmentation, vendor 0
        let apdu: &[u8] = &[
            0x10, 0x00, // Unconfirmed, I-Am
            0xC4, 0x02, 0x00, 0x04, 0xD2, // ObjectId: Device:1234
            0x22, 0x01, 0xE0, // Unsigned: 480
            0x91, 0x03, // Enumerated: 3 (no segmentation)
            0x21, 0x00, // Unsigned: 0 (vendor ID)
        ];
        let decoded = decode_apdu(apdu).unwrap();
        match decoded {
            DecodedApdu::IAm(iam) => {
                assert_eq!(iam.device_id.object_type, ObjectType::Device);
                assert_eq!(iam.device_id.instance, 1234);
                assert_eq!(iam.max_apdu, 480);
                assert_eq!(iam.segmentation, 3);
                assert_eq!(iam.vendor_id, 0);
            }
            _ => panic!("expected I-Am"),
        }
    }

    // ---- Signed integer ----

    #[test]
    fn read_property_ack_signed() {
        let ack = ReadPropertyAck {
            object_id: ObjectId::new(ObjectType::AnalogValue, 0),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::SignedInt(-42),
        };
        let mut buf = [0u8; 64];
        let n = encode_read_property_ack(&ack, 1, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::ReadPropertyAck(ack, 1));
    }

    // ---- Null value ----

    #[test]
    fn write_property_null() {
        let req = WritePropertyRequest {
            object_id: ObjectId::new(ObjectType::AnalogOutput, 0),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Null,
            priority: Some(16),
        };
        let mut buf = [0u8; 64];
        let n = encode_write_property(&req, 1, &mut buf).unwrap();
        let decoded = decode_apdu(&buf[..n]).unwrap();
        assert_eq!(decoded, DecodedApdu::WriteProperty(req, 1));
    }

    // ---- Buffer too small ----

    #[test]
    fn encode_who_is_buffer_too_small() {
        let mut buf = [0u8; 1];
        assert_eq!(
            encode_who_is(None, None, &mut buf).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    // ---- Empty APDU ----

    #[test]
    fn decode_empty() {
        assert!(decode_apdu(&[]).is_err());
    }
}

// ===========================================================================
// Cross-validation tests: Rust encode ↔ bacnet-stack C encode
// ===========================================================================

#[cfg(test)]
mod cross_validation {
    use super::*;
    use crate::bacnet::ObjectType;

    // FFI declarations for the C test harness (compiled by build.rs)
    extern "C" {
        fn bacnet_test_encode_whois(
            buf: *mut u8,
            buf_len: usize,
            low_limit: i32,
            high_limit: i32,
        ) -> i32;

        fn bacnet_test_encode_iam(
            buf: *mut u8,
            buf_len: usize,
            device_id: u32,
            max_apdu: u32,
            segmentation: u8,
            vendor_id: u16,
        ) -> i32;

        fn bacnet_test_encode_read_property(
            buf: *mut u8,
            buf_len: usize,
            invoke_id: u8,
            object_type: u16,
            object_instance: u32,
            property_id: u32,
            array_index: i32,
        ) -> i32;

        fn bacnet_test_encode_write_property_real(
            buf: *mut u8,
            buf_len: usize,
            invoke_id: u8,
            object_type: u16,
            object_instance: u32,
            property_id: u32,
            value: f32,
            priority: u8,
        ) -> i32;
    }

    /// Encode a Who-Is with bacnet-stack C, decode with our Rust.
    #[test]
    fn cross_whois_unbounded() {
        let mut c_buf = [0u8; 64];
        let c_len = unsafe { bacnet_test_encode_whois(c_buf.as_mut_ptr(), c_buf.len(), -1, -1) };
        assert!(c_len > 0, "C encode failed");

        // The C library's whois_encode_apdu returns the full APDU including
        // the 2-byte header (pdu-type + service-choice).
        let c_apdu = &c_buf[..c_len as usize];
        let decoded = decode_apdu(c_apdu).unwrap();
        assert_eq!(
            decoded,
            DecodedApdu::WhoIs(WhoIsRequest {
                low_limit: None,
                high_limit: None,
            })
        );

        // Now encode with Rust and verify byte-for-byte match
        let mut rust_buf = [0u8; 64];
        let rust_len = encode_who_is(None, None, &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Rust and C Who-Is encodings differ"
        );
    }

    #[test]
    fn cross_whois_bounded() {
        let mut c_buf = [0u8; 64];
        let c_len = unsafe { bacnet_test_encode_whois(c_buf.as_mut_ptr(), c_buf.len(), 100, 200) };
        assert!(c_len > 0, "C encode failed");

        let c_apdu = &c_buf[..c_len as usize];
        let decoded = decode_apdu(c_apdu).unwrap();
        match decoded {
            DecodedApdu::WhoIs(req) => {
                assert_eq!(req.low_limit, Some(100));
                assert_eq!(req.high_limit, Some(200));
            }
            _ => panic!("expected WhoIs, got {:?}", decoded),
        }

        // Byte-for-byte match
        let mut rust_buf = [0u8; 64];
        let rust_len = encode_who_is(Some(100), Some(200), &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Rust and C bounded Who-Is encodings differ"
        );
    }

    #[test]
    fn cross_iam() {
        let mut c_buf = [0u8; 64];
        let c_len = unsafe {
            bacnet_test_encode_iam(c_buf.as_mut_ptr(), c_buf.len(), 1234, 480, 3, 0xFFFF)
        };
        assert!(c_len > 0, "C encode failed");

        let c_apdu = &c_buf[..c_len as usize];

        // Decode C output with Rust
        let decoded = decode_apdu(c_apdu).unwrap();
        match decoded {
            DecodedApdu::IAm(iam) => {
                assert_eq!(iam.device_id.instance, 1234);
                assert_eq!(iam.device_id.object_type, ObjectType::Device);
                assert_eq!(iam.max_apdu, 480);
                assert_eq!(iam.segmentation, 3);
                assert_eq!(iam.vendor_id, 0xFFFF);
            }
            _ => panic!("expected IAm, got {:?}", decoded),
        }

        // Encode with Rust and compare
        let iam = IAmData {
            device_id: ObjectId::new(ObjectType::Device, 1234),
            max_apdu: 480,
            segmentation: 3,
            vendor_id: 0xFFFF,
        };
        let mut rust_buf = [0u8; 64];
        let rust_len = encode_i_am(&iam, &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Rust and C I-Am encodings differ"
        );
    }

    #[test]
    fn cross_read_property() {
        let mut c_buf = [0u8; 64];
        // ReadProperty: AnalogInput:1, PresentValue(85), no array index
        let c_len = unsafe {
            bacnet_test_encode_read_property(
                c_buf.as_mut_ptr(),
                c_buf.len(),
                42, // invoke_id
                0,  // AnalogInput
                1,  // instance
                85, // PresentValue
                -1, // BACNET_ARRAY_ALL = no index
            )
        };
        assert!(c_len > 0, "C encode failed");

        let c_apdu = &c_buf[..c_len as usize];

        // Decode C output with Rust
        let decoded = decode_apdu(c_apdu).unwrap();
        match decoded {
            DecodedApdu::ReadProperty(req, inv) => {
                assert_eq!(inv, 42);
                assert_eq!(req.object_id.object_type, ObjectType::AnalogInput);
                assert_eq!(req.object_id.instance, 1);
                assert_eq!(req.property_id, PropertyId::PresentValue);
                assert_eq!(req.array_index, None);
            }
            _ => panic!("expected ReadProperty, got {:?}", decoded),
        }

        // Encode with Rust and compare
        let req = ReadPropertyRequest {
            object_id: ObjectId::new(ObjectType::AnalogInput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
        };
        let mut rust_buf = [0u8; 64];
        let rust_len = encode_read_property(&req, 42, &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Rust and C ReadProperty encodings differ"
        );
    }

    #[test]
    fn cross_write_property_real() {
        let mut c_buf = [0u8; 64];
        // WriteProperty: AnalogOutput:1, PresentValue, value=72.0, priority=8
        let c_len = unsafe {
            bacnet_test_encode_write_property_real(
                c_buf.as_mut_ptr(),
                c_buf.len(),
                10,   // invoke_id
                1,    // AnalogOutput
                1,    // instance
                85,   // PresentValue
                72.0, // value
                8,    // priority
            )
        };
        assert!(c_len > 0, "C encode failed");

        let c_apdu = &c_buf[..c_len as usize];

        // Decode C output with Rust
        let decoded = decode_apdu(c_apdu).unwrap();
        match decoded {
            DecodedApdu::WriteProperty(req, inv) => {
                assert_eq!(inv, 10);
                assert_eq!(req.object_id.object_type, ObjectType::AnalogOutput);
                assert_eq!(req.object_id.instance, 1);
                assert_eq!(req.property_id, PropertyId::PresentValue);
                assert_eq!(req.value, BacnetValue::Real(72.0));
                assert_eq!(req.priority, Some(8));
            }
            _ => panic!("expected WriteProperty, got {:?}", decoded),
        }

        // Encode with Rust and compare
        let req = WritePropertyRequest {
            object_id: ObjectId::new(ObjectType::AnalogOutput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
            value: BacnetValue::Real(72.0),
            priority: Some(8),
        };
        let mut rust_buf = [0u8; 64];
        let rust_len = encode_write_property(&req, 10, &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Rust and C WriteProperty encodings differ"
        );
    }

    /// Large device instance number (22-bit boundary).
    #[test]
    fn cross_whois_large_range() {
        let mut c_buf = [0u8; 64];
        let c_len =
            unsafe { bacnet_test_encode_whois(c_buf.as_mut_ptr(), c_buf.len(), 0, 4194302) };
        assert!(c_len > 0, "C encode failed");

        let c_apdu = &c_buf[..c_len as usize];
        let decoded = decode_apdu(c_apdu).unwrap();
        match decoded {
            DecodedApdu::WhoIs(req) => {
                assert_eq!(req.low_limit, Some(0));
                assert_eq!(req.high_limit, Some(4194302));
            }
            _ => panic!("expected WhoIs"),
        }

        let mut rust_buf = [0u8; 64];
        let rust_len = encode_who_is(Some(0), Some(4194302), &mut rust_buf).unwrap();
        assert_eq!(
            &rust_buf[..rust_len],
            c_apdu,
            "Large range Who-Is encodings differ"
        );
    }
}
