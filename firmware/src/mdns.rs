//! mDNS responder task.
//!
//! Listens on UDP port 5353 and responds to:
//! - A record queries for `{hostname}.local` → our IPv4 address
//! - PTR queries for `_http._tcp.local` → service instance PTR
//! - PTR queries for `_bacnet._udp.local` → service instance PTR
//! - PTR queries for `_icomb-setup._tcp.local` → provisioning service PTR (when unprovisioned)
//! - PTR queries for `_https._tcp.local` → HTTPS service PTR (when TLS enabled)
//! - PTR queries for `_services._dns-sd._udp.local` → all advertised service types
//! - SRV queries → hostname + port
//! - TXT queries → device metadata (includes `provisioned` and `mac` for `_http._tcp`)
//!
//! Uses the `bridge_core::mdns` codec for packet encoding/decoding.

use bridge_core::mdns::{
    decode_query, encode_a_response, encode_ptr_response, encode_srv_response, encode_txt_response,
    MDNS_ADDR, MDNS_PORT, TYPE_A, TYPE_PTR, TYPE_SRV, TYPE_TXT,
};
use defmt::{info, warn};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Stack};

/// The mDNS multicast group endpoint — all responses must be sent here per
/// RFC 6762 §11.  Sending to the querier's unicast address is only allowed
/// in special "legacy unicast" cases (§6.7) which we do not implement.
const MDNS_MULTICAST_ENDPOINT: IpEndpoint = IpEndpoint::new(
    embassy_net::IpAddress::Ipv4(Ipv4Address::new(
        MDNS_ADDR[0],
        MDNS_ADDR[1],
        MDNS_ADDR[2],
        MDNS_ADDR[3],
    )),
    MDNS_PORT,
);

/// UDP packet buffer sizes.
const RX_BUF: usize = 512;
const TX_BUF: usize = 512;
const META_COUNT: usize = 4;

/// Firmware version string embedded in TXT records.
const FIRMWARE_VERSION: &str = env!("FIRMWARE_VERSION");

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
    stack.wait_config_up().await;
    info!("mdns: network up");

    let mut rx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut tx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

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
        // We do not need the sender address: all responses go to multicast (M2).
        let (n, _meta) = match socket.recv_from(&mut pkt_buf).await {
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

        // M9 TODO: decode_query() returns only the *first* question from a
        // multi-question mDNS packet (RFC 6762 §6 allows multiple questions in
        // one UDP datagram).  For the common single-question case this is fine.
        // A future improvement should iterate over all qd_count questions and
        // send a combined response, but that requires a more complex encoder.

        // Read hostname, device ID, provisioned flag, TLS state, and MAC from config.
        let (hostname, device_id, provisioned, tls_enabled, mac_hi, mac_lo) = {
            let guard = crate::http::CONFIG.lock().await;
            let (h_str, did, prov, tls) = match guard.as_ref() {
                Some(cfg) => (
                    cfg.hostname.as_str(),
                    cfg.bacnet.device_id,
                    cfg.provisioned,
                    cfg.tls.server_enabled,
                ),
                None => ("bacnet-bridge", 389999u32, false, false),
            };
            let mut h: heapless::String<32> = heapless::String::new();
            let _ = h.push_str(h_str);
            // MAC address: read the two atomic halves (HI = bytes 0-1, LO = bytes 2-5).
            let mac_hi = crate::http::MAC_ADDR_HI.load(core::sync::atomic::Ordering::Relaxed);
            let mac_lo = crate::http::MAC_ADDR_LO.load(core::sync::atomic::Ordering::Relaxed);
            (h, did, prov, tls, mac_hi, mac_lo)
        };

        // M3: Build the fully qualified name for the hostname.
        // Worst case: 32-char hostname + ".local" (6) = 38 chars → String<48>.
        let mut fqdn: heapless::String<48> = heapless::String::new();
        let _ = fqdn.push_str(hostname.as_str());
        let _ = fqdn.push_str(".local");

        // M3: Build service instance names.
        // Worst case: 32-char hostname + "._http._tcp.local" (17) = 49 chars
        //             32-char hostname + "._bacnet._udp.local" (19) = 51 chars
        //             32-char hostname + "._icomb-setup._tcp.local" (24) = 56 chars
        //             32-char hostname + "._https._tcp.local" (18) = 50 chars
        // Use String<96> to give comfortable headroom.
        let mut http_instance: heapless::String<96> = heapless::String::new();
        let _ = http_instance.push_str(hostname.as_str());
        let _ = http_instance.push_str("._http._tcp.local");

        let mut bacnet_instance: heapless::String<96> = heapless::String::new();
        let _ = bacnet_instance.push_str(hostname.as_str());
        let _ = bacnet_instance.push_str("._bacnet._udp.local");

        let mut setup_instance: heapless::String<96> = heapless::String::new();
        let _ = setup_instance.push_str(hostname.as_str());
        let _ = setup_instance.push_str("._icomb-setup._tcp.local");

        let mut https_instance: heapless::String<96> = heapless::String::new();
        let _ = https_instance.push_str(hostname.as_str());
        let _ = https_instance.push_str("._https._tcp.local");

        // Build device ID string for TXT record
        let mut device_id_str: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::write(&mut device_id_str, format_args!("{}", device_id));

        // Build mac string for TXT record (xx:xx:xx:xx:xx:xx).
        // HI = bytes 0–1 (in bits 15–0), LO = bytes 2–5 (big-endian u32).
        let mb0 = ((mac_hi >> 8) & 0xFF) as u8;
        let mb1 = (mac_hi & 0xFF) as u8;
        let mb2 = ((mac_lo >> 24) & 0xFF) as u8;
        let mb3 = ((mac_lo >> 16) & 0xFF) as u8;
        let mb4 = ((mac_lo >> 8) & 0xFF) as u8;
        let mb5 = (mac_lo & 0xFF) as u8;
        let mut mac_str: heapless::String<18> = heapless::String::new();
        let _ = core::fmt::write(
            &mut mac_str,
            format_args!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mb0, mb1, mb2, mb3, mb4, mb5,
            ),
        );

        let provisioned_str = if provisioned { "true" } else { "false" };

        let name_str = query.name.as_str();
        let qtype = query.qtype;

        // M6: For DNS-SD (_services._dns-sd._udp.local) we must send a PTR for
        // *each* service type we advertise.  Our encoder writes one record per call,
        // so we send separate UDP datagrams — all to the multicast address (M2) —
        // which is legal per RFC 6762 §11.
        if qtype == TYPE_PTR && name_str == "_services._dns-sd._udp.local" {
            let mut buf = [0u8; 512];

            // Always advertise _http._tcp and _bacnet._udp.
            let service_types: &[&str] = if !provisioned && tls_enabled {
                &[
                    "_http._tcp.local",
                    "_bacnet._udp.local",
                    "_icomb-setup._tcp.local",
                    "_https._tcp.local",
                ]
            } else if !provisioned {
                &[
                    "_http._tcp.local",
                    "_bacnet._udp.local",
                    "_icomb-setup._tcp.local",
                ]
            } else if tls_enabled {
                &[
                    "_http._tcp.local",
                    "_bacnet._udp.local",
                    "_https._tcp.local",
                ]
            } else {
                &["_http._tcp.local", "_bacnet._udp.local"]
            };

            for svc in service_types {
                if let Ok(len) = encode_ptr_response("_services._dns-sd._udp.local", svc, &mut buf)
                {
                    if let Err(_) = socket.send_to(&buf[..len], MDNS_MULTICAST_ENDPOINT).await {
                        warn!("mdns: send_to (dns-sd) failed");
                    }
                }
            }
            continue; // already handled
        }

        let resp_len = if qtype == TYPE_A && name_str == fqdn.as_str() {
            // Respond with our IPv4 address
            let ip = get_our_ip(stack);
            encode_a_response(hostname.as_str(), ip, &mut resp_buf).ok()
        } else if qtype == TYPE_PTR {
            if name_str == "_http._tcp.local" {
                encode_ptr_response("_http._tcp.local", http_instance.as_str(), &mut resp_buf).ok()
            } else if name_str == "_bacnet._udp.local" {
                encode_ptr_response(
                    "_bacnet._udp.local",
                    bacnet_instance.as_str(),
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == "_icomb-setup._tcp.local" && !provisioned {
                // Only advertise the provisioning service when not yet provisioned.
                encode_ptr_response(
                    "_icomb-setup._tcp.local",
                    setup_instance.as_str(),
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == "_https._tcp.local" && tls_enabled {
                encode_ptr_response("_https._tcp.local", https_instance.as_str(), &mut resp_buf)
                    .ok()
            } else {
                None
            }
        } else if qtype == TYPE_SRV {
            if name_str == http_instance.as_str() {
                encode_srv_response(
                    http_instance.as_str(),
                    hostname.as_str(),
                    HTTP_PORT,
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == bacnet_instance.as_str() {
                encode_srv_response(
                    bacnet_instance.as_str(),
                    hostname.as_str(),
                    BACNET_PORT,
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == setup_instance.as_str() && !provisioned {
                encode_srv_response(
                    setup_instance.as_str(),
                    hostname.as_str(),
                    HTTP_PORT,
                    &mut resp_buf,
                )
                .ok()
            } else if name_str == https_instance.as_str() && tls_enabled {
                let guard = crate::http::CONFIG.lock().await;
                let https_port = guard.as_ref().map(|c| c.tls.https_port).unwrap_or(443);
                drop(guard);
                encode_srv_response(
                    https_instance.as_str(),
                    hostname.as_str(),
                    https_port,
                    &mut resp_buf,
                )
                .ok()
            } else {
                None
            }
        } else if qtype == TYPE_TXT {
            if name_str == http_instance.as_str() {
                // _http._tcp includes provisioned and mac in TXT records.
                let txt_pairs: &[(&str, &str)] = &[
                    ("deviceId", device_id_str.as_str()),
                    ("vendor", "Icomb Place"),
                    ("version", FIRMWARE_VERSION),
                    ("provisioned", provisioned_str),
                    ("mac", mac_str.as_str()),
                ];
                encode_txt_response(name_str, txt_pairs, &mut resp_buf).ok()
            } else if name_str == bacnet_instance.as_str() {
                let txt_pairs: &[(&str, &str)] = &[
                    ("deviceId", device_id_str.as_str()),
                    ("vendor", "Icomb Place"),
                    ("version", FIRMWARE_VERSION),
                ];
                encode_txt_response(name_str, txt_pairs, &mut resp_buf).ok()
            } else if name_str == setup_instance.as_str() && !provisioned {
                let txt_pairs: &[(&str, &str)] = &[
                    ("provisioned", "false"),
                    ("vendor", "Icomb Place"),
                    ("version", FIRMWARE_VERSION),
                    ("mac", mac_str.as_str()),
                ];
                encode_txt_response(name_str, txt_pairs, &mut resp_buf).ok()
            } else if name_str == https_instance.as_str() && tls_enabled {
                let txt_pairs: &[(&str, &str)] = &[
                    ("deviceId", device_id_str.as_str()),
                    ("vendor", "Icomb Place"),
                    ("version", FIRMWARE_VERSION),
                ];
                encode_txt_response(name_str, txt_pairs, &mut resp_buf).ok()
            } else {
                None
            }
        } else {
            None
        };

        if let Some(len) = resp_len {
            // M2: mDNS responses must be sent to the multicast group (224.0.0.251:5353),
            // not back to the querier's unicast address.  RFC 6762 §11 requires this for
            // shared-resource records; sending unicast is only permitted for legacy
            // unicast queries (QU bit set, port != 5353) which we do not implement.
            if let Err(_) = socket
                .send_to(&resp_buf[..len], MDNS_MULTICAST_ENDPOINT)
                .await
            {
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
