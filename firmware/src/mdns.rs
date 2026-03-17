//! mDNS responder task.
//!
//! Listens on UDP port 5353 and responds to:
//! - A record queries for `{hostname}.local` → our IPv4 address
//! - PTR queries for `_http._tcp.local` → service instance PTR
//! - PTR queries for `_bacnet._udp.local` → service instance PTR
//! - PTR queries for `_services._dns-sd._udp.local` → both service types
//! - SRV queries → hostname + port
//! - TXT queries → device metadata
//!
//! Uses the `bridge_core::mdns` codec for packet encoding/decoding.

use bridge_core::mdns::{
    decode_query, encode_a_response, encode_ptr_response, encode_srv_response,
    encode_txt_response, MDNS_ADDR, MDNS_PORT, TYPE_A, TYPE_PTR, TYPE_SRV, TYPE_TXT,
};
use defmt::{info, warn};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Stack};

/// UDP packet buffer sizes.
const RX_BUF: usize = 512;
const TX_BUF: usize = 512;
const META_COUNT: usize = 4;

/// Firmware version string embedded in TXT records.
const FIRMWARE_VERSION: &str = "0.1.0";

/// HTTP port for SRV records.
const HTTP_PORT: u16 = 80;
/// BACnet/IP port for SRV records.
const BACNET_PORT: u16 = 47808;

/// mDNS responder task.
///
/// Binds a UDP socket to port 5353, joins the multicast group, and loops
/// processing incoming queries.
#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut tx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buf,
        &mut tx_meta,
        &mut tx_buf,
    );

    if socket.bind(MDNS_PORT).is_err() {
        warn!("mdns: bind failed");
        return;
    }

    // Join the mDNS multicast group
    let multicast_addr = IpAddress::Ipv4(Ipv4Address::new(
        MDNS_ADDR[0],
        MDNS_ADDR[1],
        MDNS_ADDR[2],
        MDNS_ADDR[3],
    ));
    if let Err(_) = stack.join_multicast_group(multicast_addr) {
        warn!("mdns: failed to join multicast group (continuing anyway)");
    }

    info!("mdns: listening on port {}", MDNS_PORT);

    let mut pkt_buf = [0u8; 512];
    let mut resp_buf = [0u8; 512];

    loop {
        let (n, meta) = match socket.recv_from(&mut pkt_buf).await {
            Ok(r) => r,
            Err(_) => {
                warn!("mdns: recv error");
                continue;
            }
        };

        let pkt = &pkt_buf[..n];
        let query = match decode_query(pkt) {
            Ok(q) => q,
            Err(_) => continue,
        };

        // Read hostname and device ID from config
        let (hostname, device_id) = {
            let guard = crate::http::CONFIG.lock().await;
            let (h_str, did) = match guard.as_ref() {
                Some(cfg) => (cfg.hostname.as_str(), cfg.bacnet.device_id),
                None => ("bacnet-bridge", 389999u32),
            };
            let mut h: heapless::String<32> = heapless::String::new();
            let _ = h.push_str(h_str);
            (h, did)
        };

        // Build the fully qualified name for the hostname
        let mut fqdn: heapless::String<64> = heapless::String::new();
        let _ = fqdn.push_str(hostname.as_str());
        let _ = fqdn.push_str(".local");

        // Build service instance names
        let mut http_instance: heapless::String<64> = heapless::String::new();
        let _ = http_instance.push_str(hostname.as_str());
        let _ = http_instance.push_str("._http._tcp.local");

        let mut bacnet_instance: heapless::String<64> = heapless::String::new();
        let _ = bacnet_instance.push_str(hostname.as_str());
        let _ = bacnet_instance.push_str("._bacnet._udp.local");

        // Build device ID string for TXT record
        let mut device_id_str: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::write(&mut device_id_str, format_args!("{}", device_id));

        let name_str = query.name.as_str();
        let qtype = query.qtype;

        let resp_len = if qtype == TYPE_A && name_str == fqdn.as_str() {
            // Respond with our IPv4 address
            let ip = get_our_ip(stack);
            encode_a_response(hostname.as_str(), ip, &mut resp_buf).ok()
        } else if qtype == TYPE_PTR {
            if name_str == "_http._tcp.local" {
                encode_ptr_response("_http._tcp.local", http_instance.as_str(), &mut resp_buf)
                    .ok()
            } else if name_str == "_bacnet._udp.local" {
                encode_ptr_response(
                    "_bacnet._udp.local",
                    bacnet_instance.as_str(),
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == "_services._dns-sd._udp.local" {
                // For DNS-SD we respond with _http._tcp.local PTR
                // (single answer — we'd need multi-answer to include both services,
                //  but our encoder only does one record per call)
                encode_ptr_response(
                    "_services._dns-sd._udp.local",
                    "_http._tcp.local",
                    &mut resp_buf,
                )
                .ok()
            } else {
                None
            }
        } else if qtype == TYPE_SRV {
            if name_str == http_instance.as_str() {
                encode_srv_response(http_instance.as_str(), hostname.as_str(), HTTP_PORT, &mut resp_buf).ok()
            } else if name_str == bacnet_instance.as_str() {
                encode_srv_response(bacnet_instance.as_str(), hostname.as_str(), BACNET_PORT, &mut resp_buf).ok()
            } else {
                None
            }
        } else if qtype == TYPE_TXT {
            let txt_pairs: &[(&str, &str)] = &[
                ("deviceId", device_id_str.as_str()),
                ("vendor", "Icomb Place"),
                ("version", FIRMWARE_VERSION),
            ];
            if name_str == http_instance.as_str() || name_str == bacnet_instance.as_str() {
                encode_txt_response(name_str, txt_pairs, &mut resp_buf).ok()
            } else {
                None
            }
        } else {
            None
        };

        if let Some(len) = resp_len {
            let remote: IpEndpoint = meta.endpoint;
            // Send response to the querying host on port 5353
            let dest = IpEndpoint::new(remote.addr, MDNS_PORT);
            if let Err(_) = socket.send_to(&resp_buf[..len], dest).await {
                warn!("mdns: send_to failed");
            }
        }
    }
}

/// Return our current IPv4 address, or [0,0,0,0] if not yet configured.
fn get_our_ip(stack: Stack<'static>) -> [u8; 4] {
    if let Some(cfg) = stack.config_v4() {
        cfg.address.address().octets()
    } else {
        [0u8; 4]
    }
}
