//! BACnet/IP UDP task (port 47808).
//!
//! Handles the BVLC encapsulation layer (ASHRAE 135 Annex J):
//! - Receives BACnet/IP packets, strips BVLC header, decodes NPDU, pushes to
//!   the `ip_to_mstp` ring buffer.
//! - Pops PDUs from `mstp_to_ip` ring buffer, encodes BVLC + NPDU, sends via UDP.
//!
//! BVLC type byte is always 0x81 (BACnet/IPv4).
//! Supported function codes:
//!   0x0A  Original-Unicast-NPDU
//!   0x0B  Original-Broadcast-NPDU

use crate::ipc;
use bridge_core::ipc::BacnetPdu;
use bridge_core::npdu::{decode_npdu, encode_npdu, NpduHeader};
use defmt::{info, warn};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpEndpoint, Ipv4Address, Stack};
use embassy_time::Timer;

/// BACnet/IP UDP port (47808 = 0xBAC0).
const BACNET_IP_PORT: u16 = 47808;

/// BVLC type byte for BACnet/IPv4.
const BVLC_TYPE: u8 = 0x81;
/// BVLC function: Original-Unicast-NPDU.
const BVLC_ORIGINAL_UNICAST: u8 = 0x0A;
/// BVLC function: Original-Broadcast-NPDU.
const BVLC_ORIGINAL_BROADCAST: u8 = 0x0B;

/// BACnet broadcast address (255.255.255.255).
const BROADCAST_ADDR: Ipv4Address = Ipv4Address::new(255, 255, 255, 255);

/// UDP receive/transmit buffer sizes.
const RX_BUF: usize = 1024;
const TX_BUF: usize = 1024;
const META_COUNT: usize = 4;

/// BACnet/IP task: receive and transmit BACnet/IP packets.
#[embassy_executor::task]
pub async fn bacnet_ip_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut tx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    if socket.bind(BACNET_IP_PORT).is_err() {
        warn!("bacnet_ip: bind failed");
        return;
    }

    info!("bacnet_ip: listening on UDP port {}", BACNET_IP_PORT);

    let mut pkt_buf = [0u8; RX_BUF];
    let mut out_buf = [0u8; TX_BUF];

    loop {
        // Check for outgoing PDUs from the MS/TP side
        let ring = ipc::mstp_to_ip();
        while let Some(pdu) = ring.pop() {
            if let Some(len) = encode_bvlc_pdu(&pdu, &mut out_buf) {
                // Determine destination: if dest_mac_len == 0 → broadcast
                let dest_ip = if pdu.dest_mac_len == 0 {
                    IpEndpoint::new(BROADCAST_ADDR.into(), BACNET_IP_PORT)
                } else if pdu.dest_mac_len >= 6 {
                    // BACnet/IP MAC = 4 bytes IP + 2 bytes port
                    let ip = Ipv4Address::new(
                        pdu.dest_mac[0],
                        pdu.dest_mac[1],
                        pdu.dest_mac[2],
                        pdu.dest_mac[3],
                    );
                    let port = ((pdu.dest_mac[4] as u16) << 8) | (pdu.dest_mac[5] as u16);
                    IpEndpoint::new(ip.into(), port)
                } else {
                    IpEndpoint::new(BROADCAST_ADDR.into(), BACNET_IP_PORT)
                };

                if let Err(_) = socket.send_to(&out_buf[..len], dest_ip).await {
                    warn!("bacnet_ip: send_to failed");
                }
            }
        }

        // Try to receive an incoming packet (non-blocking poll)
        match socket.recv_from(&mut pkt_buf).await {
            Ok((n, meta)) => {
                handle_incoming(&pkt_buf[..n], meta.endpoint);
            }
            Err(_) => {
                // No data; yield briefly before retrying
                Timer::after_millis(1).await;
            }
        }
    }
}

/// Decode an incoming BACnet/IP packet and push it onto the ip_to_mstp ring.
fn handle_incoming(data: &[u8], remote: IpEndpoint) {
    if data.len() < 6 {
        return;
    }

    let bvlc_type = data[0];
    let bvlc_func = data[1];
    let bvlc_len = ((data[2] as u16) << 8) | (data[3] as u16);

    if bvlc_type != BVLC_TYPE {
        return; // Not BACnet/IPv4
    }
    if bvlc_len as usize != data.len() {
        return; // Length mismatch
    }
    if bvlc_func != BVLC_ORIGINAL_UNICAST && bvlc_func != BVLC_ORIGINAL_BROADCAST {
        // We only handle original unicast/broadcast for now
        return;
    }

    let npdu_data = &data[4..];
    let (npdu_hdr, apdu) = match decode_npdu(npdu_data) {
        Ok(r) => r,
        Err(_) => return,
    };

    if apdu.len() > bridge_core::ipc::PDU_MAX_DATA {
        warn!("bacnet_ip: apdu too large ({} bytes), dropping", apdu.len());
        return;
    }

    let mut pdu = BacnetPdu::new();

    // Fill source address from remote IP:port
    match remote.addr {
        embassy_net::IpAddress::Ipv4(ipv4) => {
            let octets = ipv4.octets();
            pdu.source_mac[0] = octets[0];
            pdu.source_mac[1] = octets[1];
            pdu.source_mac[2] = octets[2];
            pdu.source_mac[3] = octets[3];
            pdu.source_mac[4] = (remote.port >> 8) as u8;
            pdu.source_mac[5] = remote.port as u8;
            pdu.source_mac_len = 6;
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }

    pdu.source_net = npdu_hdr.src_net;
    pdu.dest_net = npdu_hdr.dest_net;

    if npdu_hdr.dest_present {
        let dlen = npdu_hdr.dest_mac_len as usize;
        pdu.dest_mac[..dlen].copy_from_slice(&npdu_hdr.dest_mac[..dlen]);
        pdu.dest_mac_len = npdu_hdr.dest_mac_len;
    }

    pdu.pdu_type = if apdu.is_empty() { 0xFF } else { apdu[0] };
    pdu.data_len = apdu.len() as u16;
    pdu.data[..apdu.len()].copy_from_slice(apdu);

    let ring = ipc::ip_to_mstp();
    if !ring.push(&pdu) {
        warn!("bacnet_ip: ip_to_mstp ring full, dropping PDU");
    }
}

/// Encode a `BacnetPdu` into a BVLC + NPDU packet in `buf`.
/// Returns the number of bytes written, or `None` on error.
fn encode_bvlc_pdu(pdu: &BacnetPdu, buf: &mut [u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }

    // Build NPDU header from the PDU fields
    let mut npdu_hdr = NpduHeader::local(false);
    npdu_hdr.dest_net = pdu.dest_net;
    if pdu.dest_mac_len > 0 {
        npdu_hdr.dest_present = true;
        npdu_hdr.dest_mac_len = pdu.dest_mac_len;
        npdu_hdr.dest_mac = pdu.dest_mac;
        npdu_hdr.hop_count = 0xFF;
    }
    npdu_hdr.src_net = pdu.source_net;
    if pdu.source_mac_len > 0 {
        npdu_hdr.src_present = true;
        npdu_hdr.src_mac_len = pdu.source_mac_len;
        npdu_hdr.src_mac = pdu.source_mac;
    }

    let apdu = &pdu.data[..pdu.data_len as usize];

    // Leave room for 4-byte BVLC header
    let npdu_len = match encode_npdu(&npdu_hdr, apdu, &mut buf[4..]) {
        Ok(n) => n,
        Err(_) => return None,
    };

    let total_len = 4 + npdu_len;

    // BVLC header
    buf[0] = BVLC_TYPE;
    buf[1] = if pdu.dest_mac_len == 0 {
        BVLC_ORIGINAL_BROADCAST
    } else {
        BVLC_ORIGINAL_UNICAST
    };
    buf[2] = (total_len >> 8) as u8;
    buf[3] = total_len as u8;

    Some(total_len)
}
