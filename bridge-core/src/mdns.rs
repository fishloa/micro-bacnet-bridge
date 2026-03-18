//! Minimal mDNS packet codec.
//!
//! Supports encoding A, PTR, SRV, and TXT resource records, and decoding
//! incoming mDNS queries. All encoding writes uncompressed DNS name labels.
//! Decoding follows name pointers (compression) up to a small depth limit.

use crate::error::{DecodeError, EncodeError};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// mDNS multicast UDP port.
pub const MDNS_PORT: u16 = 5353;
/// mDNS multicast group address.
pub const MDNS_ADDR: [u8; 4] = [224, 0, 0, 251];

/// DNS record type: A (IPv4 address).
pub const TYPE_A: u16 = 1;
/// DNS record type: PTR (pointer / reverse lookup).
pub const TYPE_PTR: u16 = 12;
/// DNS record type: TXT.
pub const TYPE_TXT: u16 = 16;
/// DNS record type: SRV.
pub const TYPE_SRV: u16 = 33;

/// DNS class: IN (Internet), with cache-flush bit set (0x8001).
pub const CLASS_IN_FLUSH: u16 = 0x8001;
/// DNS class: IN without cache-flush.
pub const CLASS_IN: u16 = 0x0001;

/// Default mDNS TTL for resource records (75% of 1 hour as recommended).
pub const MDNS_TTL: u32 = 4500;

// ---------------------------------------------------------------------------
// DNS packet header
// ---------------------------------------------------------------------------

/// A DNS/mDNS packet header (12 bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qd_count: u16,
    pub an_count: u16,
    pub ns_count: u16,
    pub ar_count: u16,
}

impl DnsHeader {
    /// Create a standard mDNS *response* header (QR=1, AA=1).
    pub fn response(an_count: u16) -> Self {
        Self {
            id: 0,
            flags: 0x8400, // QR=1, AA=1
            qd_count: 0,
            an_count,
            ns_count: 0,
            ar_count: 0,
        }
    }

    fn encode(&self, buf: &mut [u8], pos: &mut usize) -> Result<(), EncodeError> {
        if buf.len() < *pos + 12 {
            return Err(EncodeError::BufferTooSmall);
        }
        write_u16(buf, pos, self.id);
        write_u16(buf, pos, self.flags);
        write_u16(buf, pos, self.qd_count);
        write_u16(buf, pos, self.an_count);
        write_u16(buf, pos, self.ns_count);
        write_u16(buf, pos, self.ar_count);
        Ok(())
    }

    fn decode(data: &[u8]) -> Result<(Self, usize), DecodeError> {
        if data.len() < 12 {
            return Err(DecodeError::UnexpectedEnd);
        }
        let hdr = DnsHeader {
            id: read_u16(data, 0),
            flags: read_u16(data, 2),
            qd_count: read_u16(data, 4),
            an_count: read_u16(data, 6),
            ns_count: read_u16(data, 8),
            ar_count: read_u16(data, 10),
        };
        Ok((hdr, 12))
    }
}

// ---------------------------------------------------------------------------
// Resource record
// ---------------------------------------------------------------------------

/// A parsed DNS resource record (name + type/class/ttl/rdata).
#[derive(Debug, Clone)]
pub struct ResourceRecord<'a> {
    /// The DNS name, as a raw label sequence (not decoded to a string here).
    pub name_end: usize,
    pub rr_type: u16,
    pub class: u16,
    pub ttl: u32,
    pub rdata: &'a [u8],
}

// ---------------------------------------------------------------------------
// DNS name helpers
// ---------------------------------------------------------------------------

/// Write a DNS name from a dot-separated host string plus an optional domain
/// suffix (e.g. "local"). The name is written as uncompressed labels and
/// terminated with a 0x00 length byte.
///
/// Example: `write_name(buf, pos, "bacnet-bridge", "local")` writes
/// `\x0dbacnet-bridge\x05local\x00`.
fn write_name(
    buf: &mut [u8],
    pos: &mut usize,
    name: &str,
    domain: &str,
) -> Result<(), EncodeError> {
    for part in name.split('.') {
        write_label(buf, pos, part)?;
    }
    if !domain.is_empty() {
        for part in domain.split('.') {
            write_label(buf, pos, part)?;
        }
    }
    if *pos >= buf.len() {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[*pos] = 0x00;
    *pos += 1;
    Ok(())
}

/// Write a single DNS label (length byte + bytes).
fn write_label(buf: &mut [u8], pos: &mut usize, label: &str) -> Result<(), EncodeError> {
    let bytes = label.as_bytes();
    let len = bytes.len();
    if len > 63 {
        return Err(EncodeError::StringTooLong);
    }
    if buf.len() < *pos + 1 + len {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[*pos] = len as u8;
    *pos += 1;
    buf[*pos..*pos + len].copy_from_slice(bytes);
    *pos += len;
    Ok(())
}

/// Read a DNS name from `data` starting at `start`, following compression
/// pointers. Returns the decoded name as a heapless String<128> and the
/// byte offset just *after* the name (not following any pointer target).
///
/// We limit pointer following to 8 hops to prevent infinite loops.
fn read_name(data: &[u8], start: usize) -> Result<(heapless::String<128>, usize), DecodeError> {
    let mut name: heapless::String<128> = heapless::String::new();
    let mut pos = start;
    let mut end_pos: Option<usize> = None;
    let mut hops = 0usize;

    loop {
        if pos >= data.len() {
            return Err(DecodeError::UnexpectedEnd);
        }
        let len = data[pos];
        if len == 0x00 {
            // End of name
            if end_pos.is_none() {
                end_pos = Some(pos + 1);
            }
            break;
        } else if (len & 0xC0) == 0xC0 {
            // Compression pointer
            if pos + 1 >= data.len() {
                return Err(DecodeError::UnexpectedEnd);
            }
            let offset = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            if end_pos.is_none() {
                end_pos = Some(pos + 2);
            }
            if offset >= data.len() {
                return Err(DecodeError::InvalidNamePointer);
            }
            hops += 1;
            if hops > 8 {
                return Err(DecodeError::InvalidNamePointer);
            }
            pos = offset;
        } else {
            // Regular label
            let label_len = len as usize;
            pos += 1;
            if pos + label_len > data.len() {
                return Err(DecodeError::UnexpectedEnd);
            }
            // Add separator dot if not the first label
            if !name.is_empty() {
                name.push('.').map_err(|_| DecodeError::InvalidData)?;
            }
            let label_bytes = &data[pos..pos + label_len];
            for &b in label_bytes {
                name.push(b as char).map_err(|_| DecodeError::InvalidData)?;
            }
            pos += label_len;
        }
    }

    Ok((name, end_pos.unwrap_or(pos + 1)))
}

// ---------------------------------------------------------------------------
// RR type/class/ttl/rdlength helpers
// ---------------------------------------------------------------------------

fn write_rr_header(
    buf: &mut [u8],
    pos: &mut usize,
    rr_type: u16,
    class: u16,
    ttl: u32,
    rdlength: u16,
) -> Result<(), EncodeError> {
    if buf.len() < *pos + 10 {
        return Err(EncodeError::BufferTooSmall);
    }
    write_u16(buf, pos, rr_type);
    write_u16(buf, pos, class);
    write_u32(buf, pos, ttl);
    write_u16(buf, pos, rdlength);
    Ok(())
}

// ---------------------------------------------------------------------------
// Integer helpers (big-endian)
// ---------------------------------------------------------------------------

#[inline]
fn write_u16(buf: &mut [u8], pos: &mut usize, val: u16) {
    buf[*pos] = (val >> 8) as u8;
    buf[*pos + 1] = val as u8;
    *pos += 2;
}

#[inline]
fn write_u32(buf: &mut [u8], pos: &mut usize, val: u32) {
    buf[*pos] = (val >> 24) as u8;
    buf[*pos + 1] = (val >> 16) as u8;
    buf[*pos + 2] = (val >> 8) as u8;
    buf[*pos + 3] = val as u8;
    *pos += 4;
}

#[inline]
fn read_u16(data: &[u8], pos: usize) -> u16 {
    ((data[pos] as u16) << 8) | (data[pos + 1] as u16)
}

// ---------------------------------------------------------------------------
// Public encode functions
// ---------------------------------------------------------------------------

/// Encode a DNS A record mDNS response.
///
/// Writes a single-answer mDNS response resolving `hostname` (without `.local`)
/// to the given IPv4 `ip` address. `.local` is appended automatically.
pub fn encode_a_response(
    hostname: &str,
    ip: [u8; 4],
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    let hdr = DnsHeader::response(1);
    hdr.encode(buf, &mut pos)?;
    // Name: hostname.local
    write_name(buf, &mut pos, hostname, "local")?;
    write_rr_header(buf, &mut pos, TYPE_A, CLASS_IN_FLUSH, MDNS_TTL, 4)?;
    if buf.len() < pos + 4 {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[pos..pos + 4].copy_from_slice(&ip);
    pos += 4;
    Ok(pos)
}

/// Encode a DNS PTR record mDNS response.
///
/// Used to advertise a service instance. E.g. `service` = `_http._tcp.local`,
/// `instance` = `My Bridge._http._tcp.local`.
pub fn encode_ptr_response(
    service: &str,
    instance: &str,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    let hdr = DnsHeader::response(1);
    hdr.encode(buf, &mut pos)?;

    // Name: service (e.g. _http._tcp.local)
    write_name(buf, &mut pos, service, "")?;

    // Calculate rdata length: instance name as DNS labels
    // We write to a scratch area to compute the length, then do it again.
    // Since we can't alloc, we compute inline by writing to a fixed temp offset.
    // Strategy: remember pos before rdata, write placeholder length, write rdata,
    // then backfill the length.
    let rdlength_pos = pos + 8; // after type(2)+class(2)+ttl(4)
    write_rr_header(buf, &mut pos, TYPE_PTR, CLASS_IN, MDNS_TTL, 0)?;
    let rdata_start = pos;
    write_name(buf, &mut pos, instance, "")?;
    let rdlength = (pos - rdata_start) as u16;
    buf[rdlength_pos] = (rdlength >> 8) as u8;
    buf[rdlength_pos + 1] = rdlength as u8;
    Ok(pos)
}

/// Encode a DNS SRV record mDNS response.
///
/// `instance` is the fully-qualified service instance name (e.g.
/// `My Bridge._http._tcp.local`). `hostname` is the target host (without
/// `.local`; `.local` is appended automatically).
pub fn encode_srv_response(
    instance: &str,
    hostname: &str,
    port: u16,
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    let hdr = DnsHeader::response(1);
    hdr.encode(buf, &mut pos)?;

    write_name(buf, &mut pos, instance, "")?;

    // rdata: priority(2) + weight(2) + port(2) + target name
    // Backfill rdlength trick
    let rdlength_pos = pos + 8;
    write_rr_header(buf, &mut pos, TYPE_SRV, CLASS_IN_FLUSH, MDNS_TTL, 0)?;
    let rdata_start = pos;
    // priority = 0, weight = 0
    if buf.len() < pos + 6 {
        return Err(EncodeError::BufferTooSmall);
    }
    write_u16(buf, &mut pos, 0); // priority
    write_u16(buf, &mut pos, 0); // weight
    write_u16(buf, &mut pos, port);
    write_name(buf, &mut pos, hostname, "local")?;
    let rdlength = (pos - rdata_start) as u16;
    buf[rdlength_pos] = (rdlength >> 8) as u8;
    buf[rdlength_pos + 1] = rdlength as u8;
    Ok(pos)
}

/// Encode a DNS TXT record mDNS response.
///
/// `txt` is a slice of `(key, value)` pairs; each pair is encoded as
/// `"key=value"` in the TXT rdata. Empty-value pairs are encoded as just the
/// key.
pub fn encode_txt_response(
    instance: &str,
    txt: &[(&str, &str)],
    buf: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut pos = 0;
    let hdr = DnsHeader::response(1);
    hdr.encode(buf, &mut pos)?;

    write_name(buf, &mut pos, instance, "")?;

    let rdlength_pos = pos + 8;
    write_rr_header(buf, &mut pos, TYPE_TXT, CLASS_IN_FLUSH, MDNS_TTL, 0)?;
    let rdata_start = pos;

    for (key, val) in txt {
        // Compute length of "key=value" or "key"
        let pair_len = if val.is_empty() {
            key.len()
        } else {
            key.len() + 1 + val.len()
        };
        if pair_len > 255 {
            return Err(EncodeError::StringTooLong);
        }
        if buf.len() < pos + 1 + pair_len {
            return Err(EncodeError::BufferTooSmall);
        }
        buf[pos] = pair_len as u8;
        pos += 1;
        let key_bytes = key.as_bytes();
        buf[pos..pos + key_bytes.len()].copy_from_slice(key_bytes);
        pos += key_bytes.len();
        if !val.is_empty() {
            buf[pos] = b'=';
            pos += 1;
            let val_bytes = val.as_bytes();
            buf[pos..pos + val_bytes.len()].copy_from_slice(val_bytes);
            pos += val_bytes.len();
        }
    }

    let rdlength = (pos - rdata_start) as u16;
    buf[rdlength_pos] = (rdlength >> 8) as u8;
    buf[rdlength_pos + 1] = rdlength as u8;
    Ok(pos)
}

// ---------------------------------------------------------------------------
// DnsQuery
// ---------------------------------------------------------------------------

/// A parsed mDNS question entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DnsQuery {
    /// Fully-qualified name being queried (dot-separated, max 128 bytes).
    pub name: heapless::String<128>,
    /// DNS query type (e.g. TYPE_A, TYPE_PTR, TYPE_SRV, TYPE_TXT).
    pub qtype: u16,
    /// DNS query class (e.g. CLASS_IN).
    pub qclass: u16,
}

/// Decode the *first* question from an mDNS query packet.
///
/// Returns the parsed `DnsQuery`. Multi-question packets are supported at the
/// caller level — this function returns the first question only, which is the
/// common case for mDNS.
pub fn decode_query(data: &[u8]) -> Result<DnsQuery, DecodeError> {
    let (hdr, mut pos) = DnsHeader::decode(data)?;

    if hdr.qd_count == 0 {
        return Err(DecodeError::InvalidData);
    }

    let (name, new_pos) = read_name(data, pos)?;
    pos = new_pos;

    if data.len() < pos + 4 {
        return Err(DecodeError::UnexpectedEnd);
    }
    let qtype = read_u16(data, pos);
    let qclass = read_u16(data, pos + 2);

    Ok(DnsQuery {
        name,
        qtype,
        qclass,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- A record ----------------------------------------------------------

    #[test]
    fn encode_a_response_basic() {
        let mut buf = [0u8; 256];
        let n = encode_a_response("mydevice", [192, 168, 1, 42], &mut buf).unwrap();
        assert!(n > 12); // at least header + name + rdata
                         // Check last 4 bytes are the IP
        assert_eq!(&buf[n - 4..n], &[192, 168, 1, 42]);
        // Verify header: QR=1, AA=1, an_count=1
        assert_eq!(read_u16(&buf, 2), 0x8400);
        assert_eq!(read_u16(&buf, 6), 1); // an_count
    }

    #[test]
    fn encode_a_buffer_too_small() {
        let mut buf = [0u8; 10]; // way too small
        assert_eq!(
            encode_a_response("mydevice", [10, 0, 0, 1], &mut buf).unwrap_err(),
            EncodeError::BufferTooSmall
        );
    }

    // ---- PTR record --------------------------------------------------------

    #[test]
    fn encode_ptr_response_basic() {
        let mut buf = [0u8; 256];
        let n = encode_ptr_response(
            "_http._tcp.local",
            "BACnet Bridge._http._tcp.local",
            &mut buf,
        )
        .unwrap();
        assert!(n > 12);
        // an_count should be 1
        assert_eq!(read_u16(&buf, 6), 1);
    }

    // ---- SRV record --------------------------------------------------------

    #[test]
    fn encode_srv_response_basic() {
        let mut buf = [0u8; 256];
        let n = encode_srv_response(
            "BACnet Bridge._http._tcp.local",
            "bacnet-bridge",
            80,
            &mut buf,
        )
        .unwrap();
        assert!(n > 12);
    }

    // ---- TXT record --------------------------------------------------------

    #[test]
    fn encode_txt_response_basic() {
        let mut buf = [0u8; 256];
        let pairs: &[(&str, &str)] = &[
            ("deviceId", "389999"),
            ("vendor", "Icomb Place"),
            ("version", "0.1.0"),
        ];
        let n = encode_txt_response("BACnet Bridge._bacnet._udp.local", pairs, &mut buf).unwrap();
        assert!(n > 12);
        // Verify the TXT rdata contains the key=value pairs
        // Find "deviceId=389999" as a string in the buffer
        let raw = &buf[..n];
        // The TXT data should appear somewhere after the header+name+rr-header
        let _haystack = core::str::from_utf8(raw).unwrap_or("");
        // Not purely UTF-8 due to length bytes, so search by bytes
        let target = b"deviceId=389999";
        let found = raw.windows(target.len()).any(|w| w == target);
        assert!(found, "TXT rdata should contain 'deviceId=389999'");
    }

    // ---- decode_query ------------------------------------------------------

    #[test]
    fn decode_query_type_a() {
        // Build a minimal mDNS query for "bacnet-bridge.local" TYPE_A
        let mut buf = [0u8; 256];
        let mut pos = 0usize;
        // Header: id=0, flags=0 (query), qd=1
        write_u16(&mut buf, &mut pos, 0); // id
        write_u16(&mut buf, &mut pos, 0); // flags
        write_u16(&mut buf, &mut pos, 1); // qd_count
        write_u16(&mut buf, &mut pos, 0); // an_count
        write_u16(&mut buf, &mut pos, 0); // ns_count
        write_u16(&mut buf, &mut pos, 0); // ar_count
                                          // Name: bacnet-bridge.local
        write_name(&mut buf, &mut pos, "bacnet-bridge", "local").unwrap();
        // qtype = A, qclass = IN
        write_u16(&mut buf, &mut pos, TYPE_A);
        write_u16(&mut buf, &mut pos, CLASS_IN);

        let q = decode_query(&buf[..pos]).unwrap();
        assert_eq!(q.name.as_str(), "bacnet-bridge.local");
        assert_eq!(q.qtype, TYPE_A);
        assert_eq!(q.qclass, CLASS_IN);
    }

    #[test]
    fn decode_query_type_ptr() {
        let mut buf = [0u8; 256];
        let mut pos = 0usize;
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 1);
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        write_name(&mut buf, &mut pos, "_http._tcp", "local").unwrap();
        write_u16(&mut buf, &mut pos, TYPE_PTR);
        write_u16(&mut buf, &mut pos, CLASS_IN);

        let q = decode_query(&buf[..pos]).unwrap();
        assert_eq!(q.qtype, TYPE_PTR);
    }

    #[test]
    fn decode_query_no_questions_error() {
        let mut buf = [0u8; 12];
        let mut pos = 0usize;
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0); // qd_count = 0
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        write_u16(&mut buf, &mut pos, 0);
        assert_eq!(
            decode_query(&buf[..12]).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn decode_query_too_short() {
        assert_eq!(decode_query(&[]).unwrap_err(), DecodeError::UnexpectedEnd);
        assert_eq!(
            decode_query(&[0u8; 5]).unwrap_err(),
            DecodeError::UnexpectedEnd
        );
    }

    #[test]
    fn constants_are_correct() {
        assert_eq!(MDNS_PORT, 5353);
        assert_eq!(MDNS_ADDR, [224, 0, 0, 251]);
        assert_eq!(TYPE_A, 1);
        assert_eq!(TYPE_PTR, 12);
        assert_eq!(TYPE_TXT, 16);
        assert_eq!(TYPE_SRV, 33);
    }

    #[test]
    fn encode_a_then_decode_response() {
        // Encode a response
        let mut buf = [0u8; 256];
        let n = encode_a_response("bridge", [10, 0, 1, 2], &mut buf).unwrap();
        // Manually verify structure: header 12 bytes, then name "bridge.local",
        // then type/class/ttl/rdlen/rdata.
        // Header flags should be response (QR=1)
        assert_ne!(read_u16(&buf, 2) & 0x8000, 0, "QR bit should be set");
        // Verify RDATA (last 4 bytes)
        assert_eq!(&buf[n - 4..n], &[10, 0, 1, 2]);
    }
}
