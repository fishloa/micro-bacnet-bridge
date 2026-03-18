//! DNS stub resolver task.
//!
//! Resolves A records (IPv4) by sending a UDP query to the configured DNS
//! server (from `BridgeConfig::network::dns`) or falling back to `8.8.8.8`
//! when no server is configured or config is not yet loaded.
//!
//! # Usage
//! ```rust,ignore
//! let ip = dns::resolve(stack, "pool.ntp.org").await;
//! ```
//!
//! # Design
//! - Opens a fresh UDP socket per call (syslog-style, low frequency assumed).
//! - Uses a simple global `AtomicU16` counter for transaction IDs.
//! - Retries up to `MAX_ATTEMPTS` times with a `ATTEMPT_TIMEOUT` per attempt.
//! - Returns `None` on any permanent failure (NXDOMAIN, timeout, etc.).

use bridge_core::dns_client::{decode_response, encode_query, DNS_PORT};
use defmt::warn;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpEndpoint, Ipv4Address, Stack};
use embassy_time::{with_timeout, Duration};
use portable_atomic::{AtomicU16, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default DNS server when no server is configured (Google Public DNS).
const FALLBACK_DNS: Ipv4Address = Ipv4Address::new(8, 8, 8, 8);

/// Number of query attempts per resolve call.
const MAX_ATTEMPTS: usize = 3;

/// Per-attempt receive timeout.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

/// UDP buffer sizes.
const RX_BUF: usize = 512;
const TX_BUF: usize = 512;
const META_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// Transaction ID counter
// ---------------------------------------------------------------------------

/// Monotonically incrementing transaction ID counter.
///
/// Wraps on overflow (u16 range is sufficient — DNS IDs only need uniqueness
/// within the outstanding request window, which here is always 1).
static TX_ID_COUNTER: AtomicU16 = AtomicU16::new(1);

/// Allocate the next DNS transaction ID (1-based, wraps at u16::MAX).
fn next_tx_id() -> u16 {
    // fetch_add wraps naturally on overflow for AtomicU16
    let id = TX_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Avoid 0 (some resolvers treat ID=0 as invalid)
    if id == 0 {
        1
    } else {
        id
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve a hostname to an IPv4 address.
///
/// Queries the DNS server configured in `BridgeConfig::network::dns`; falls
/// back to `8.8.8.8` if the config is not yet available.
///
/// Returns `Some([u8; 4])` on success, `None` if all attempts fail.
pub async fn resolve(stack: Stack<'static>, hostname: &str) -> Option<[u8; 4]> {
    // Determine which DNS server to use
    let dns_ip = {
        let guard = crate::http::CONFIG.lock().await;
        match guard.as_ref() {
            Some(cfg) => {
                let d = cfg.network.dns;
                // If the configured DNS is all-zero, use the fallback
                if d == [0u8; 4] {
                    FALLBACK_DNS
                } else {
                    Ipv4Address::new(d[0], d[1], d[2], d[3])
                }
            }
            None => FALLBACK_DNS,
        }
    };

    let server_endpoint = IpEndpoint::new(dns_ip.into(), DNS_PORT);

    let mut rx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut tx_meta = [PacketMetadata::EMPTY; META_COUNT];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    if socket.bind(0).is_err() {
        warn!("dns: failed to bind UDP socket");
        return None;
    }

    let tx_id = next_tx_id();
    let mut query_buf = [0u8; 512];

    let query_len = match encode_query(&mut query_buf, tx_id, hostname) {
        Ok(n) => n,
        Err(_) => {
            warn!("dns: failed to encode query for hostname");
            return None;
        }
    };

    for attempt in 0..MAX_ATTEMPTS {
        if socket
            .send_to(&query_buf[..query_len], server_endpoint)
            .await
            .is_err()
        {
            warn!("dns: send failed (attempt {})", attempt + 1);
            continue;
        }

        let mut resp_buf = [0u8; RX_BUF];
        let recv_result = with_timeout(ATTEMPT_TIMEOUT, socket.recv_from(&mut resp_buf)).await;

        match recv_result {
            Ok(Ok((n, _meta))) => match decode_response(&resp_buf[..n], tx_id) {
                Ok(ip) => return Some(ip),
                Err(_) => {
                    warn!("dns: bad or NXDOMAIN response (attempt {})", attempt + 1);
                }
            },
            Ok(Err(_)) => {
                warn!("dns: recv error (attempt {})", attempt + 1);
            }
            Err(_timeout) => {
                warn!("dns: timeout (attempt {})", attempt + 1);
            }
        }
    }

    None
}
