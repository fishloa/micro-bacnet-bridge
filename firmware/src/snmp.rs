//! Minimal read-only SNMP v2c agent task (RFC 3416).
//!
//! Listens on UDP port 161 and handles GetRequest and GetNextRequest PDUs.
//! SetRequest is silently ignored (read-only agent). Only the community
//! string `"public"` is accepted.
//!
//! # Supported OIDs
//! ## System MIB (RFC 1213 / RFC 3418)
//! | OID                  | Value                                |
//! |----------------------|--------------------------------------|
//! | sysDescr  (1.1.1.0)  | "BACnet Bridge vX.Y.Z (Icomb Place)" |
//! | sysUpTime (1.1.3.0)  | Timeticks since boot (centiseconds)  |
//! | sysContact(1.1.4.0)  | "Icomb Place"                        |
//! | sysName   (1.1.5.0)  | Device hostname from config          |
//!
//! ## Custom enterprise OIDs (enterprises.99999.1.N)
//! | OID | Value |
//! |-----|-------|
//! | .1.1 | mstpFramesSent (Counter32) |
//! | .1.2 | mstpFramesRecv (Counter32) |
//! | .1.3 | mstpTokenLosses (Counter32) |
//! | .1.4 | ipcDropCount (Counter32) |
//! | .1.5 | bacnetDevicesDiscovered (Gauge32) |
//!
//! # Notes
//! - Only one OID per request is processed (the first). Multi-OID requests
//!   return a response with `noSuchName` for any OID beyond the first that
//!   is not found (standard v2c behaviour). In practice most NMS tools send
//!   one OID per GetRequest.
//! - GetNextRequest does a naive linear scan of the known OID table.
//! - `sysUpTime` is derived from [`embassy_time::Instant::now`].

use bridge_core::snmp::{
    decode_get_request, encode_get_response, SnmpValue, VarBind, ERROR_NO_ERROR,
    ERROR_NO_SUCH_NAME, OID_BACNET_DEVICES_DISCOVERED, OID_IPC_DROP_COUNT, OID_MSTP_FRAMES_RECV,
    OID_MSTP_FRAMES_SENT, OID_MSTP_TOKEN_LOSSES, OID_SYS_CONTACT, OID_SYS_DESCR, OID_SYS_NAME,
    OID_SYS_UPTIME, TAG_GET_NEXT_REQUEST,
};
use defmt::{info, warn};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpEndpoint, Stack};
use embassy_time::Instant;
use heapless::Vec;
use portable_atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Bridge-wide counters (written by other tasks, read by SNMP)
// ---------------------------------------------------------------------------

/// Number of MS/TP frames sent by Core 1 (incremented by C side via FFI).
pub static MSTP_FRAMES_SENT: AtomicU32 = AtomicU32::new(0);
/// Number of MS/TP frames received by Core 1.
pub static MSTP_FRAMES_RECV: AtomicU32 = AtomicU32::new(0);
/// Number of MS/TP token losses detected.
pub static MSTP_TOKEN_LOSSES: AtomicU32 = AtomicU32::new(0);
/// Number of PDUs dropped due to full IPC ring buffers.
pub static IPC_DROP_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// SNMP UDP port.
const SNMP_PORT: u16 = 161;

/// Accepted community string (read-only agent).
const COMMUNITY: &[u8] = b"public";

/// Firmware version string (embedded at compile time).
const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// UDP buffer sizes.
const RX_BUF: usize = 512;
const TX_BUF: usize = 512;
const META_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// Known OID table (for GetNextRequest)
//
// Listed in lexicographic order so GetNext can return the successor OID.
// ---------------------------------------------------------------------------

const OID_TABLE: &[&[u32]] = &[
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

// ---------------------------------------------------------------------------
// SNMP agent task
// ---------------------------------------------------------------------------

/// SNMP v2c agent task.
///
/// Binds UDP port 161 and services GetRequest / GetNextRequest PDUs.
/// Runs forever; does not return.
#[embassy_executor::task]
pub async fn snmp_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut tx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    if socket.bind(SNMP_PORT).is_err() {
        warn!("snmp: bind on port {} failed", SNMP_PORT);
        return;
    }

    info!("snmp: listening on UDP port {}", SNMP_PORT);

    let mut pkt_buf = [0u8; RX_BUF];
    let mut resp_buf = [0u8; TX_BUF];

    loop {
        let (n, meta) = match socket.recv_from(&mut pkt_buf).await {
            Ok(r) => r,
            Err(_) => {
                warn!("snmp: recv error");
                continue;
            }
        };

        let pkt = &pkt_buf[..n];

        // Decode the SNMP request
        let req = match decode_get_request(pkt) {
            Ok(r) => r,
            Err(_) => {
                // Silently drop malformed or unsupported packets
                continue;
            }
        };

        // Check community string — reject anything that isn't "public"
        if req.community.as_slice() != COMMUNITY {
            warn!("snmp: rejected unknown community");
            continue;
        }

        // Build variable bindings for the response
        let bindings = build_bindings(&req.oids, req.pdu_type).await;

        // Determine error status: if any requested OID wasn't found,
        // set noSuchName for the first missing one.
        let (error_status, error_index) = if bindings.len() < req.oids.len() {
            // At least one OID was not found — report the first missing index.
            // error_index is 1-based per RFC.
            let first_missing = req
                .oids
                .iter()
                .enumerate()
                .find(|(i, oid)| {
                    if *i < bindings.len() {
                        bindings[*i].oid.as_slice() != oid.as_slice()
                    } else {
                        true
                    }
                })
                .map(|(i, _)| (i + 1) as i32)
                .unwrap_or(1);
            (ERROR_NO_SUCH_NAME, first_missing)
        } else {
            (ERROR_NO_ERROR, 0)
        };

        // Encode and send the GetResponse
        let resp_len = match encode_get_response(
            &mut resp_buf,
            req.request_id,
            COMMUNITY,
            error_status,
            error_index,
            &bindings,
        ) {
            Ok(n) => n,
            Err(_) => {
                warn!("snmp: encode_get_response failed");
                continue;
            }
        };

        let sender: IpEndpoint = meta.endpoint;
        if let Err(_) = socket.send_to(&resp_buf[..resp_len], sender).await {
            warn!("snmp: send_to failed");
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build variable bindings for the given OIDs.
///
/// For GetRequest: looks up each OID directly.
/// For GetNextRequest: for each requested OID, finds and returns the
/// lexicographically next OID in our table.
///
/// OIDs not found in the table are silently omitted from the result.
async fn build_bindings(
    oids: &heapless::Vec<heapless::Vec<u32, 16>, 8>,
    pdu_type: u8,
) -> heapless::Vec<VarBind, 8> {
    let mut bindings: heapless::Vec<VarBind, 8> = heapless::Vec::new();

    for requested_oid in oids.iter() {
        let lookup_oid: &[u32] = if pdu_type == TAG_GET_NEXT_REQUEST {
            // Find the OID that lexicographically follows the requested OID
            match next_oid(requested_oid.as_slice()) {
                Some(o) => o,
                None => continue, // past end of our MIB
            }
        } else {
            requested_oid.as_slice()
        };

        if let Some(vb) = resolve_oid(lookup_oid).await {
            let _ = bindings.push(vb);
        }
    }

    bindings
}

/// Return the OID from our table that immediately follows `oid` in
/// lexicographic order, or `None` if `oid` is already at or past the end.
fn next_oid(oid: &[u32]) -> Option<&'static [u32]> {
    // Find the first entry in OID_TABLE that is strictly greater than `oid`
    for &table_oid in OID_TABLE {
        if oid_cmp(table_oid, oid) == core::cmp::Ordering::Greater {
            return Some(table_oid);
        }
    }
    None
}

/// Lexicographic comparison of two OID slices.
fn oid_cmp(a: &[u32], b: &[u32]) -> core::cmp::Ordering {
    let len = a.len().min(b.len());
    for i in 0..len {
        match a[i].cmp(&b[i]) {
            core::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

/// Resolve an OID to its current value and return a `VarBind`, or `None`
/// if the OID is not in our MIB.
async fn resolve_oid(oid: &[u32]) -> Option<VarBind> {
    // Match against known OIDs
    let value = if oid == OID_SYS_DESCR {
        // "BACnet Bridge vX.Y.Z (Icomb Place)"
        let mut s: Vec<u8, 64> = Vec::new();
        for &b in b"BACnet Bridge v" {
            let _ = s.push(b);
        }
        for &b in FIRMWARE_VERSION.as_bytes() {
            let _ = s.push(b);
        }
        for &b in b" (Icomb Place)" {
            let _ = s.push(b);
        }
        SnmpValue::OctetString(s)
    } else if oid == OID_SYS_UPTIME {
        // Centiseconds since boot
        let now_ms = Instant::now().as_millis();
        let centisecs = (now_ms / 10) as u32;
        SnmpValue::TimeTicks(centisecs)
    } else if oid == OID_SYS_CONTACT {
        let mut s: Vec<u8, 64> = Vec::new();
        for &b in b"Icomb Place" {
            let _ = s.push(b);
        }
        SnmpValue::OctetString(s)
    } else if oid == OID_SYS_NAME {
        // Read hostname from config
        let guard = crate::http::CONFIG.lock().await;
        let hostname = match guard.as_ref() {
            Some(cfg) => cfg.hostname.as_str(),
            None => "bacnet-bridge",
        };
        let mut s: Vec<u8, 64> = Vec::new();
        for &b in hostname.as_bytes() {
            let _ = s.push(b);
        }
        SnmpValue::OctetString(s)
    } else if oid == OID_MSTP_FRAMES_SENT {
        SnmpValue::Counter32(MSTP_FRAMES_SENT.load(Ordering::Relaxed))
    } else if oid == OID_MSTP_FRAMES_RECV {
        SnmpValue::Counter32(MSTP_FRAMES_RECV.load(Ordering::Relaxed))
    } else if oid == OID_MSTP_TOKEN_LOSSES {
        SnmpValue::Counter32(MSTP_TOKEN_LOSSES.load(Ordering::Relaxed))
    } else if oid == OID_IPC_DROP_COUNT {
        SnmpValue::Counter32(IPC_DROP_COUNT.load(Ordering::Relaxed))
    } else if oid == OID_BACNET_DEVICES_DISCOVERED {
        // Count entries in the bridge state device table
        let count = {
            let guard = crate::bridge::BRIDGE_STATE.lock().await;
            guard.devices.iter().filter(|d| d.device_id != 0).count() as u32
        };
        SnmpValue::Gauge32(count)
    } else {
        return None;
    };

    // Build VarBind with the looked-up OID
    let mut oid_vec: Vec<u32, 16> = Vec::new();
    for &s in oid {
        let _ = oid_vec.push(s);
    }

    Some(VarBind {
        oid: oid_vec,
        value,
    })
}
