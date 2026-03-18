//! DNS resolver task.
//!
//! Resolves A records (IPv4) using embassy-net's built-in DNS client
//! (`Stack::dns_query` with the `"dns"` feature enabled on embassy-net).
//!
//! The DNS server list is configured automatically by DHCP, or can be
//! overridden by setting `dns_servers` on the stack config when using a static
//! IP.  No manual UDP socket management or packet codec is required.
//!
//! # Usage
//! ```rust,ignore
//! let ip = dns::resolve(stack, "pool.ntp.org").await;
//! ```
//!
//! # Design
//! - One `Stack::dns_query` call per resolve; embassy-net serialises concurrent
//!   queries internally using its DNS socket slot pool.
//! - Retries up to `MAX_ATTEMPTS` times on transient failure.
//! - Returns `None` on any permanent or repeated failure.

use defmt::warn;
use embassy_net::dns::DnsQueryType;
use embassy_net::{IpAddress, Stack};
use embassy_time::{with_timeout, Duration};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of query attempts per resolve call.
const MAX_ATTEMPTS: usize = 3;

/// Per-attempt timeout.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve a hostname to an IPv4 address using embassy-net's DNS client.
///
/// Returns `Some([u8; 4])` on success, `None` if all attempts fail or no A
/// record is present in the response.
pub async fn resolve(stack: Stack<'static>, hostname: &str) -> Option<[u8; 4]> {
    for attempt in 0..MAX_ATTEMPTS {
        let result =
            with_timeout(ATTEMPT_TIMEOUT, stack.dns_query(hostname, DnsQueryType::A)).await;

        match result {
            Ok(Ok(addrs)) => {
                // Extract the first address; we queried A records so all
                // results should be IPv4, but we filter defensively.
                let ipv4 = addrs.iter().find_map(|addr| match *addr {
                    IpAddress::Ipv4(v4) => Some(v4.octets()),
                    _ => None,
                });
                if let Some(octets) = ipv4 {
                    return Some(octets);
                }
                warn!(
                    "dns: no A record in response for {} (attempt {})",
                    hostname,
                    attempt + 1
                );
            }
            Ok(Err(_e)) => {
                warn!(
                    "dns: query failed for {} (attempt {})",
                    hostname,
                    attempt + 1
                );
            }
            Err(_timeout) => {
                warn!("dns: timeout for {} (attempt {})", hostname, attempt + 1);
            }
        }
    }

    None
}
