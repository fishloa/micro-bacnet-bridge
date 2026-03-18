//! DNS stub resolver codec (RFC 1035 — A record queries only).
//!
//! Provides a minimal, allocation-free DNS client codec for resolving A
//! (IPv4 address) records. This is not a full resolver; it encodes a single
//! question query and decodes the first A record from the answer section.
//!
//! # Usage
//!
//! 1. Call [`encode_query`] to build a DNS query packet to send over UDP to
//!    port 53.
//! 2. Receive the response from the DNS server.
//! 3. Call [`decode_response`] with the same `id` used when encoding to
//!    extract the IPv4 address.
//!
//! This module is `no_std` and allocates nothing.

use crate::error::{DecodeError, EncodeError};
use crate::mdns::{read_name, read_u16, write_name, write_u16};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// DNS standard UDP port.
pub const DNS_PORT: u16 = 53;

/// DNS record type: A (IPv4 address).
const TYPE_A: u16 = 1;

/// DNS class: IN (Internet).
const CLASS_IN: u16 = 1;

/// DNS RCODE mask (lower 4 bits of flags word).
const RCODE_MASK: u16 = 0x000F;

/// QR bit: set in responses.
const FLAG_QR: u16 = 0x8000;

/// RD bit: recursion desired (set in queries).
const FLAG_RD: u16 = 0x0100;

// ---------------------------------------------------------------------------
// encode_query
// ---------------------------------------------------------------------------

/// Build a DNS query packet for an A record lookup and write it into `buf`.
///
/// Encodes a standard recursive DNS query (QR=0, RD=1) with one question:
/// `hostname IN A`.
///
/// # Arguments
/// - `buf` — output buffer; must be large enough to hold the 12-byte header
///   plus the encoded question. 512 bytes is always sufficient for any valid
///   hostname.
/// - `id` — 16-bit transaction identifier. The caller must supply the same
///   value to [`decode_response`] to verify the reply.
/// - `hostname` — fully-qualified (or plain) hostname to look up, e.g.
///   `"pool.ntp.org"`. Must not end with a trailing dot; the root label is
///   added automatically.
///
/// # Returns
/// The number of bytes written into `buf`, or [`EncodeError::BufferTooSmall`]
/// / [`EncodeError::StringTooLong`] on failure.
pub fn encode_query(buf: &mut [u8], id: u16, hostname: &str) -> Result<usize, EncodeError> {
    if buf.len() < 12 {
        return Err(EncodeError::BufferTooSmall);
    }

    let mut pos = 0usize;

    // --- DNS header (12 bytes) ---
    write_u16(buf, &mut pos, id); // ID
    write_u16(buf, &mut pos, FLAG_RD); // Flags: QR=0 (query), RD=1
    write_u16(buf, &mut pos, 1); // QDCOUNT = 1
    write_u16(buf, &mut pos, 0); // ANCOUNT = 0
    write_u16(buf, &mut pos, 0); // NSCOUNT = 0
    write_u16(buf, &mut pos, 0); // ARCOUNT = 0

    // --- Question section ---
    // Name: write all labels from hostname; domain suffix is empty (we write
    // the full hostname including any dots as separate labels).
    write_name(buf, &mut pos, hostname, "")?;

    // QTYPE = A (1)
    if buf.len() < pos + 4 {
        return Err(EncodeError::BufferTooSmall);
    }
    write_u16(buf, &mut pos, TYPE_A);
    // QCLASS = IN (1)
    write_u16(buf, &mut pos, CLASS_IN);

    Ok(pos)
}

// ---------------------------------------------------------------------------
// decode_response
// ---------------------------------------------------------------------------

/// Parse a DNS response and extract the first A record IP address.
///
/// # Arguments
/// - `data` — raw bytes received from the DNS server on port 53.
/// - `expected_id` — must match the transaction ID in the response header.
///
/// # Returns
/// The IPv4 address from the first A record in the answer section, or a
/// [`DecodeError`] if:
/// - `data` is too short to contain a valid DNS header
///   ([`DecodeError::UnexpectedEnd`]).
/// - The transaction ID does not match `expected_id`
///   ([`DecodeError::InvalidData`]).
/// - The RCODE is non-zero (e.g. NXDOMAIN = 3)
///   ([`DecodeError::InvalidData`]).
/// - The answer count is zero
///   ([`DecodeError::InvalidData`]).
/// - No A record is found in the answer section (wrong type/class or
///   truncated) ([`DecodeError::InvalidData`]).
pub fn decode_response(data: &[u8], expected_id: u16) -> Result<[u8; 4], DecodeError> {
    if data.len() < 12 {
        return Err(DecodeError::UnexpectedEnd);
    }

    let id = read_u16(data, 0);
    if id != expected_id {
        return Err(DecodeError::InvalidData);
    }

    let flags = read_u16(data, 2);

    // Must be a response (QR bit set)
    if flags & FLAG_QR == 0 {
        return Err(DecodeError::InvalidData);
    }

    // Check RCODE (lower 4 bits of flags)
    let rcode = flags & RCODE_MASK;
    if rcode != 0 {
        return Err(DecodeError::InvalidData);
    }

    let qd_count = read_u16(data, 4);
    let an_count = read_u16(data, 6);

    if an_count == 0 {
        return Err(DecodeError::InvalidData);
    }

    // Skip past the header
    let mut pos = 12usize;

    // Skip the question section (qd_count questions)
    for _ in 0..qd_count {
        // Skip the question name
        let (_, new_pos) = read_name(data, pos)?;
        pos = new_pos;
        // Skip QTYPE (2) + QCLASS (2)
        if data.len() < pos + 4 {
            return Err(DecodeError::UnexpectedEnd);
        }
        pos += 4;
    }

    // Parse answer records, looking for the first TYPE_A / CLASS_IN record
    for _ in 0..an_count {
        // Read answer name (may use compression pointers)
        let (_, new_pos) = read_name(data, pos)?;
        pos = new_pos;

        // Need at least type(2) + class(2) + ttl(4) + rdlength(2) = 10 bytes
        if data.len() < pos + 10 {
            return Err(DecodeError::UnexpectedEnd);
        }
        let rr_type = read_u16(data, pos);
        let rr_class = read_u16(data, pos + 2);
        // ttl at pos+4..pos+8 — skipped
        let rdlength = read_u16(data, pos + 8) as usize;
        pos += 10;

        if data.len() < pos + rdlength {
            return Err(DecodeError::UnexpectedEnd);
        }

        if rr_type == TYPE_A && rr_class == CLASS_IN {
            // A record rdata must be exactly 4 bytes
            if rdlength < 4 {
                return Err(DecodeError::InvalidData);
            }
            let ip = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
            return Ok(ip);
        }

        // Not the record we want — skip rdata
        pos += rdlength;
    }

    // Walked all answers, no A record found
    Err(DecodeError::InvalidData)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers to build test DNS packets
    // -----------------------------------------------------------------------

    /// Write big-endian u32 into buf at pos and advance pos.
    fn put_u32(buf: &mut [u8], pos: &mut usize, val: u32) {
        buf[*pos] = (val >> 24) as u8;
        buf[*pos + 1] = (val >> 16) as u8;
        buf[*pos + 2] = (val >> 8) as u8;
        buf[*pos + 3] = val as u8;
        *pos += 4;
    }

    /// Write big-endian u16 into buf at pos and advance pos.
    fn put_u16(buf: &mut [u8], pos: &mut usize, val: u16) {
        buf[*pos] = (val >> 8) as u8;
        buf[*pos + 1] = val as u8;
        *pos += 2;
    }

    /// Write a DNS-encoded name (labels + terminating zero) at pos.
    fn put_name(buf: &mut [u8], pos: &mut usize, labels: &[&str]) {
        for label in labels {
            let len = label.len() as u8;
            buf[*pos] = len;
            *pos += 1;
            buf[*pos..*pos + label.len()].copy_from_slice(label.as_bytes());
            *pos += label.len();
        }
        buf[*pos] = 0x00; // root label
        *pos += 1;
    }

    /// Build a complete minimal DNS response for a single A record.
    ///
    /// - id: transaction ID
    /// - flags: DNS flags word (caller sets QR, RD, RA, RCODE etc.)
    /// - qd_count: number of questions to include in the response
    /// - an_count: number of answer records to include
    /// - question_name: labels for the question section name
    /// - answers: list of (type, class, ttl, rdata) for each answer
    fn build_response(
        id: u16,
        flags: u16,
        question_name: &[&str],
        answers: &[(u16, u16, u32, &[u8])],
    ) -> ([u8; 512], usize) {
        let mut buf = [0u8; 512];
        let mut pos = 0usize;

        let qd_count: u16 = if question_name.is_empty() { 0 } else { 1 };
        let an_count: u16 = answers.len() as u16;

        // Header
        put_u16(&mut buf, &mut pos, id);
        put_u16(&mut buf, &mut pos, flags);
        put_u16(&mut buf, &mut pos, qd_count);
        put_u16(&mut buf, &mut pos, an_count);
        put_u16(&mut buf, &mut pos, 0); // NSCOUNT
        put_u16(&mut buf, &mut pos, 0); // ARCOUNT

        // Question section
        if !question_name.is_empty() {
            put_name(&mut buf, &mut pos, question_name);
            put_u16(&mut buf, &mut pos, TYPE_A); // QTYPE
            put_u16(&mut buf, &mut pos, CLASS_IN); // QCLASS
        }

        // Answer section
        for (rr_type, rr_class, ttl, rdata) in answers {
            // Name: use a compression pointer back to the question name at
            // offset 12 (just after the header), if there is a question.
            // Otherwise write a minimal root label.
            if !question_name.is_empty() {
                buf[pos] = 0xC0; // pointer high byte
                buf[pos + 1] = 0x0C; // offset 12
                pos += 2;
            } else {
                buf[pos] = 0x00; // root label
                pos += 1;
            }
            put_u16(&mut buf, &mut pos, *rr_type);
            put_u16(&mut buf, &mut pos, *rr_class);
            put_u32(&mut buf, &mut pos, *ttl);
            put_u16(&mut buf, &mut pos, rdata.len() as u16);
            buf[pos..pos + rdata.len()].copy_from_slice(rdata);
            pos += rdata.len();
        }

        (buf, pos)
    }

    // -----------------------------------------------------------------------
    // encode_query tests
    // -----------------------------------------------------------------------

    /// Verify that `encode_query("pool.ntp.org", ...)` produces a packet with
    /// the correct DNS header fields and encodes "pool.ntp.org" as DNS labels.
    #[test]
    fn test_encode_query_pool_ntp_org() {
        let mut buf = [0u8; 128];
        let id: u16 = 0xABCD;
        let n = encode_query(&mut buf, id, "pool.ntp.org").unwrap();

        // Minimum size: 12 (header) + 5+4+3+1 (pool.ntp.org labels+lens+zero)
        // + 4 (QTYPE + QCLASS) = 12 + 13 + 4 = 29 bytes
        assert!(n >= 29, "encoded packet too short: {} bytes", n);

        // Header: ID
        assert_eq!(read_u16(&buf, 0), id, "ID mismatch");

        // Header: Flags — QR=0 (query), RD=1
        let flags = read_u16(&buf, 2);
        assert_eq!(flags & FLAG_QR, 0, "QR must be 0 for a query");
        assert_ne!(flags & FLAG_RD, 0, "RD must be set");

        // QDCOUNT = 1
        assert_eq!(read_u16(&buf, 4), 1, "QDCOUNT must be 1");

        // ANCOUNT, NSCOUNT, ARCOUNT = 0
        assert_eq!(read_u16(&buf, 6), 0, "ANCOUNT must be 0");
        assert_eq!(read_u16(&buf, 8), 0, "NSCOUNT must be 0");
        assert_eq!(read_u16(&buf, 10), 0, "ARCOUNT must be 0");

        // Question section starts at byte 12.
        // "pool.ntp.org" encodes as:
        //   \x04 p o o l  \x03 n t p  \x03 o r g  \x00
        let q = &buf[12..];
        assert_eq!(q[0], 4, "first label length must be 4 ('pool')");
        assert_eq!(&q[1..5], b"pool", "first label must be 'pool'");
        assert_eq!(q[5], 3, "second label length must be 3 ('ntp')");
        assert_eq!(&q[6..9], b"ntp", "second label must be 'ntp'");
        assert_eq!(q[9], 3, "third label length must be 3 ('org')");
        assert_eq!(&q[10..13], b"org", "third label must be 'org'");
        assert_eq!(q[13], 0, "root label (terminating zero) must be present");

        // QTYPE = TYPE_A (1) and QCLASS = CLASS_IN (1) follow the name
        let qtype_offset = 12 + 4 + 1 + 3 + 1 + 3 + 1 + 1; // = 26
        assert_eq!(read_u16(&buf, qtype_offset), TYPE_A, "QTYPE must be A (1)");
        assert_eq!(
            read_u16(&buf, qtype_offset + 2),
            CLASS_IN,
            "QCLASS must be IN (1)"
        );
    }

    /// Buffer too small for the DNS header must return BufferTooSmall.
    #[test]
    fn test_encode_query_buffer_too_small() {
        let mut buf = [0u8; 8];
        let err = encode_query(&mut buf, 1, "a.b").unwrap_err();
        assert_eq!(err, EncodeError::BufferTooSmall);
    }

    // -----------------------------------------------------------------------
    // decode_response tests
    // -----------------------------------------------------------------------

    /// A well-formed response containing a single A record must return the
    /// correct IP address.
    #[test]
    fn test_decode_response_single_a_record() {
        let ip = [93, 184, 216, 34]; // example.com
        let (pkt, n) = build_response(
            0x1234,
            FLAG_QR | FLAG_RD | 0x0080, // QR=1, RD=1, RA=1, RCODE=0
            &["example", "com"],
            &[(TYPE_A, CLASS_IN, 300, &ip)],
        );
        let result = decode_response(&pkt[..n], 0x1234).unwrap();
        assert_eq!(result, ip);
    }

    /// Verify that multiple answer records are handled: the first A record is
    /// returned even when preceded by records of other types.
    #[test]
    fn test_decode_response_first_a_record_returned() {
        let ip = [1, 2, 3, 4];
        // Two A records; function should return the first one.
        let ip2 = [5, 6, 7, 8];
        let (pkt, n) = build_response(
            0xBEEF,
            FLAG_QR,
            &["pool", "ntp", "org"],
            &[(TYPE_A, CLASS_IN, 60, &ip), (TYPE_A, CLASS_IN, 60, &ip2)],
        );
        let result = decode_response(&pkt[..n], 0xBEEF).unwrap();
        assert_eq!(result, ip, "first A record must be returned");
    }

    /// A wrong transaction ID must return InvalidData.
    #[test]
    fn test_decode_response_wrong_id() {
        let ip = [10, 0, 0, 1];
        let (pkt, n) = build_response(0xAAAA, FLAG_QR, &["host"], &[(TYPE_A, CLASS_IN, 60, &ip)]);
        let err = decode_response(&pkt[..n], 0xBBBB).unwrap_err();
        assert_eq!(
            err,
            DecodeError::InvalidData,
            "wrong ID must return InvalidData"
        );
    }

    /// RCODE=3 (NXDOMAIN) must return InvalidData.
    #[test]
    fn test_decode_response_nxdomain() {
        // RCODE = 3 (NXDOMAIN), QR=1, no answers
        let (pkt, n) = build_response(
            0x0001,
            FLAG_QR | 0x0003, // RCODE=3
            &["nonexistent", "example"],
            &[], // no answers
        );
        let err = decode_response(&pkt[..n], 0x0001).unwrap_err();
        assert_eq!(
            err,
            DecodeError::InvalidData,
            "NXDOMAIN must return InvalidData"
        );
    }

    /// A response with ANCOUNT=0 must return InvalidData.
    #[test]
    fn test_decode_response_no_answers() {
        let (pkt, n) = build_response(
            0x5555,
            FLAG_QR, // RCODE=0 but no answers
            &["norecord", "example"],
            &[],
        );
        let err = decode_response(&pkt[..n], 0x5555).unwrap_err();
        assert_eq!(
            err,
            DecodeError::InvalidData,
            "zero answers must return InvalidData"
        );
    }

    /// Data shorter than 12 bytes must return UnexpectedEnd.
    #[test]
    fn test_decode_response_truncated_header() {
        let err = decode_response(&[0u8; 10], 0x1234).unwrap_err();
        assert_eq!(err, DecodeError::UnexpectedEnd);
    }

    /// An empty slice must return UnexpectedEnd.
    #[test]
    fn test_decode_response_empty() {
        let err = decode_response(&[], 0x0000).unwrap_err();
        assert_eq!(err, DecodeError::UnexpectedEnd);
    }

    /// A packet that claims to be a query (QR=0) must return InvalidData.
    #[test]
    fn test_decode_response_rejects_query_packet() {
        let (pkt, n) = build_response(
            0x0001,
            0x0000, // QR=0 (query, not response)
            &["example"],
            &[(TYPE_A, CLASS_IN, 60, &[1u8, 2, 3, 4])],
        );
        let err = decode_response(&pkt[..n], 0x0001).unwrap_err();
        assert_eq!(
            err,
            DecodeError::InvalidData,
            "query packet must be rejected"
        );
    }

    /// Verify correct encoding of a single-label hostname.
    #[test]
    fn test_encode_query_single_label() {
        let mut buf = [0u8; 64];
        let n = encode_query(&mut buf, 0x0001, "localhost").unwrap();
        // "localhost" → \x09localhost\x00 = 11 bytes
        // header(12) + name(11) + qtype(2) + qclass(2) = 27
        assert_eq!(n, 27);
        assert_eq!(buf[12], 9, "label length must be 9");
        assert_eq!(&buf[13..22], b"localhost");
        assert_eq!(buf[22], 0, "terminating zero");
    }

    /// DNS port constant must be 53.
    #[test]
    fn test_dns_port_constant() {
        assert_eq!(DNS_PORT, 53);
    }
}
