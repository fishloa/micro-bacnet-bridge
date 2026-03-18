//! Inter-core communication types and ring buffer.
//!
//! The ring buffer is an SPSC (single-producer, single-consumer) design using
//! volatile reads/writes for index synchronisation. This avoids any dependency
//! on `portable_atomic` (which doesn't support thumbv6m) while remaining
//! correct when used strictly as SPSC across the two RP2040 cores.
//!
//! **Safety contract**: `push` must only be called from one core/context and
//! `pop` from the other. Violating this is undefined behaviour.

use core::ptr::{read_volatile, write_volatile};

// ---------------------------------------------------------------------------
// BacnetPdu
// ---------------------------------------------------------------------------

/// Maximum APDU size supported by the bridge.
/// BACnet/IP supports up to 1497 bytes but we budget for MS/TP max of 480.
pub const PDU_MAX_DATA: usize = 480;

/// A BACnet PDU passed between Core 0 and Core 1 via the shared ring buffer.
///
/// `repr(C)` is required so the C code on Core 1 can access the same struct.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct BacnetPdu {
    /// Source network number (0 = local MS/TP segment).
    pub source_net: u16,
    /// Source MAC address bytes.
    pub source_mac: [u8; 7],
    /// Number of valid bytes in `source_mac`.
    pub source_mac_len: u8,
    /// Destination network number.
    pub dest_net: u16,
    /// Destination MAC address bytes.
    pub dest_mac: [u8; 7],
    /// Number of valid bytes in `dest_mac`.
    pub dest_mac_len: u8,
    /// PDU type byte (APDU type nibble, or 0xFF for network-layer messages).
    pub pdu_type: u8,
    /// Number of valid bytes in `data`.
    pub data_len: u16,
    /// PDU payload (APDU bytes).
    pub data: [u8; PDU_MAX_DATA],
}

// Manual Debug to avoid printing 480 bytes
impl core::fmt::Debug for BacnetPdu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BacnetPdu")
            .field("source_net", &self.source_net)
            .field("source_mac_len", &self.source_mac_len)
            .field("dest_net", &self.dest_net)
            .field("dest_mac_len", &self.dest_mac_len)
            .field("pdu_type", &self.pdu_type)
            .field("data_len", &self.data_len)
            .finish()
    }
}

impl PartialEq for BacnetPdu {
    fn eq(&self, other: &Self) -> bool {
        self.source_net == other.source_net
            && self.source_mac == other.source_mac
            && self.source_mac_len == other.source_mac_len
            && self.dest_net == other.dest_net
            && self.dest_mac == other.dest_mac
            && self.dest_mac_len == other.dest_mac_len
            && self.pdu_type == other.pdu_type
            && self.data_len == other.data_len
            && self.data[..self.data_len as usize] == other.data[..other.data_len as usize]
    }
}

impl BacnetPdu {
    /// Create a zeroed PDU.
    pub const fn new() -> Self {
        Self {
            source_net: 0,
            source_mac: [0u8; 7],
            source_mac_len: 0,
            dest_net: 0,
            dest_mac: [0u8; 7],
            dest_mac_len: 0,
            pdu_type: 0,
            data_len: 0,
            data: [0u8; PDU_MAX_DATA],
        }
    }
}

impl Default for BacnetPdu {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RingBuffer
// ---------------------------------------------------------------------------

/// Lock-free SPSC ring buffer holding up to `N` `BacnetPdu` items.
///
/// `N` must be a power of two for the index-masking optimisation to work,
/// though this is not enforced at compile time — callers should choose powers
/// of two (e.g. 4, 8, 16).
///
/// `repr(C)` ensures the field layout matches the C `ipc_ring_t`:
/// `head` (u32), `tail` (u32), `buffer` ([bacnet_pdu_t; N])`.
#[repr(C)]
pub struct RingBuffer<const N: usize> {
    /// Write index (producer increments). Volatile access required.
    head: u32,
    /// Read index (consumer increments). Volatile access required.
    tail: u32,
    /// Storage for PDUs.
    data: [BacnetPdu; N],
}

impl<const N: usize> RingBuffer<N> {
    // Ensure N is non-zero (checked at compile time via array size).
    const _CHECK: () = assert!(N > 0, "RingBuffer size N must be > 0");

    /// Create an empty ring buffer.
    pub const fn new() -> Self {
        Self {
            head: 0,
            tail: 0,
            data: [BacnetPdu::new(); N],
        }
    }

    /// Returns true if the buffer contains no items.
    #[inline]
    pub fn is_empty(&self) -> bool {
        // SAFETY: self is exclusively accessed here (SPSC contract).
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        h == t
    }

    /// Returns true if the buffer is full and cannot accept another item.
    #[inline]
    pub fn is_full(&self) -> bool {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        (h.wrapping_add(1)) & (N as u32 - 1) == t
    }

    /// Push a PDU into the buffer (producer side).
    ///
    /// Returns `true` on success, `false` if the buffer is full.
    pub fn push(&mut self, pdu: &BacnetPdu) -> bool {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        let next_h = h.wrapping_add(1) & (N as u32 - 1);
        if next_h == t {
            return false; // full
        }
        self.data[h as usize] = *pdu;
        // Write head last (release semantics via volatile write)
        unsafe { write_volatile(&mut self.head, next_h) };
        true
    }

    /// Pop a PDU from the buffer (consumer side).
    ///
    /// Returns `Some(pdu)` if an item was available, `None` if empty.
    pub fn pop(&mut self) -> Option<BacnetPdu> {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        if h == t {
            return None; // empty
        }
        let pdu = self.data[t as usize];
        let next_t = t.wrapping_add(1) & (N as u32 - 1);
        // Advance tail last (release semantics via volatile write)
        unsafe { write_volatile(&mut self.tail, next_t) };
        Some(pdu)
    }

    /// Return the number of items currently in the buffer.
    pub fn len(&self) -> usize {
        let h = unsafe { read_volatile(&self.head) } as usize;
        let t = unsafe { read_volatile(&self.tail) } as usize;
        if h >= t {
            h - t
        } else {
            N - t + h
        }
    }
}

impl<const N: usize> Default for RingBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pdu(pdu_type: u8, data_byte: u8) -> BacnetPdu {
        let mut pdu = BacnetPdu::new();
        pdu.pdu_type = pdu_type;
        pdu.data[0] = data_byte;
        pdu.data_len = 1;
        pdu
    }

    #[test]
    fn new_buffer_is_empty() {
        let rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn push_one_pop_one() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        let pdu = make_pdu(0x10, 0xAB);
        assert!(rb.push(&pdu));
        assert!(!rb.is_empty());
        assert_eq!(rb.len(), 1);
        let out = rb.pop().unwrap();
        assert_eq!(out.pdu_type, 0x10);
        assert_eq!(out.data[0], 0xAB);
        assert_eq!(out.data_len, 1);
        assert!(rb.is_empty());
    }

    #[test]
    fn fill_and_drain() {
        // RingBuffer<4> can hold 3 items (capacity = N-1 due to full/empty distinction)
        let mut rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.push(&make_pdu(1, 0x01)));
        assert!(rb.push(&make_pdu(2, 0x02)));
        assert!(rb.push(&make_pdu(3, 0x03)));
        assert!(rb.is_full());
        assert!(!rb.push(&make_pdu(4, 0x04))); // should fail

        let p1 = rb.pop().unwrap();
        let p2 = rb.pop().unwrap();
        let p3 = rb.pop().unwrap();
        assert_eq!(p1.pdu_type, 1);
        assert_eq!(p2.pdu_type, 2);
        assert_eq!(p3.pdu_type, 3);
        assert!(rb.is_empty());
        assert!(rb.pop().is_none());
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.pop().is_none());
    }

    #[test]
    fn wraps_around_correctly() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        // Fill, drain, fill again — exercises index wrap
        for i in 0..3u8 {
            rb.push(&make_pdu(i, i));
        }
        for _ in 0..3 {
            rb.pop().unwrap();
        }
        // Now fill again past the original end
        for i in 10..13u8 {
            rb.push(&make_pdu(i, i));
        }
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 10);
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 11);
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 12);
        assert!(rb.is_empty());
    }

    #[test]
    fn push_returns_false_when_full() {
        let mut rb: RingBuffer<2> = RingBuffer::new();
        // Capacity of RingBuffer<2> = 1 item
        assert!(rb.push(&make_pdu(1, 1)));
        assert!(rb.is_full());
        assert!(!rb.push(&make_pdu(2, 2)));
    }

    #[test]
    fn pdu_equality() {
        let a = make_pdu(0x10, 0xFF);
        let b = make_pdu(0x10, 0xFF);
        assert_eq!(a, b);
        let c = make_pdu(0x20, 0xFF);
        assert_ne!(a, c);
    }

    #[test]
    fn len_matches_pushes_minus_pops() {
        let mut rb: RingBuffer<8> = RingBuffer::new();
        assert_eq!(rb.len(), 0);
        rb.push(&make_pdu(1, 1));
        assert_eq!(rb.len(), 1);
        rb.push(&make_pdu(2, 2));
        assert_eq!(rb.len(), 2);
        rb.pop();
        assert_eq!(rb.len(), 1);
        rb.pop();
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn pdu_data_payload() {
        let mut pdu = BacnetPdu::new();
        pdu.source_net = 42;
        pdu.dest_net = 100;
        pdu.pdu_type = 0x30;
        pdu.data_len = 3;
        pdu.data[0] = 0xAA;
        pdu.data[1] = 0xBB;
        pdu.data[2] = 0xCC;

        let mut rb: RingBuffer<4> = RingBuffer::new();
        rb.push(&pdu);
        let out = rb.pop().unwrap();
        assert_eq!(out.source_net, 42);
        assert_eq!(out.dest_net, 100);
        assert_eq!(out.data_len, 3);
        assert_eq!(&out.data[..3], &[0xAA, 0xBB, 0xCC]);
    }
}
