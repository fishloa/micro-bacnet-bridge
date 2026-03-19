//! NTP client task (SNTP, RFC 4330).
//!
//! Synchronises the bridge clock against a public NTP server on boot and then
//! every hour. The resolved UTC epoch is stored in a shared [`AtomicU32`]
//! (`UNIX_EPOCH_SECS`) so other tasks can read the current time without
//! waiting for a lock.
//!
//! # Shared state
//! - [`UNIX_EPOCH_SECS`] — current Unix timestamp (seconds since 1970-01-01).
//!   0 = not yet synced. Other tasks call [`unix_now`] to read it.
//! - [`NTP_SYNCED`] — `true` once at least one successful sync has occurred.
//!
//! # NTP server
//! DNS resolution is not available in this firmware (no resolver task). We use
//! the Cloudflare time service (162.159.200.1) which is reliable and anycast.
//! The IP is hardcoded per the requirement; a future config option could allow
//! overriding this from the stored `BridgeConfig`.
//!
//! # Retry policy
//! Each sync attempt sends up to 3 requests, each with a 2-second timeout.
//! Between sync cycles the task sleeps for `SYNC_INTERVAL_SECS` (3600 s).
//! On startup the first sync is attempted before the interval sleep begins.

use bridge_core::ntp::{decode_response, encode_request, ntp_to_unix_epoch, NTP_PORT};
use defmt::{info, warn};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpEndpoint, Ipv4Address, Stack};
use embassy_time::{with_timeout, Duration, Timer};
use portable_atomic::{AtomicBool, AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Current Unix timestamp in seconds (0 = not yet synced).
///
/// Written exclusively by [`ntp_task`]; read by any task via [`unix_now`].
static UNIX_EPOCH_SECS: AtomicU32 = AtomicU32::new(0);

/// Set to `true` after the first successful NTP sync.
static NTP_SYNCED: AtomicBool = AtomicBool::new(false);

/// Return the current Unix timestamp (seconds since 1970-01-01), or `None`
/// if NTP has not yet been synchronised.
#[allow(dead_code)]
pub fn unix_now() -> Option<u32> {
    let v = UNIX_EPOCH_SECS.load(Ordering::Relaxed);
    if v == 0 {
        None
    } else {
        Some(v)
    }
}

/// Return `true` if at least one NTP sync has completed successfully.
#[allow(dead_code)]
pub fn is_synced() -> bool {
    NTP_SYNCED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Cloudflare time (time.cloudflare.com anycast) — 162.159.200.1.
/// Used because we have no DNS resolver in firmware.
const NTP_SERVER_IP: Ipv4Address = Ipv4Address::new(162, 159, 200, 1);

/// How often to re-sync after the first successful sync (seconds).
const SYNC_INTERVAL_SECS: u64 = 3600;

/// Per-attempt receive timeout.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum number of send/receive attempts per sync cycle.
const MAX_ATTEMPTS: usize = 3;

/// UDP receive/transmit metadata and buffer sizes for the NTP socket.
const RX_META: usize = 2;
const TX_META: usize = 2;
const RX_BUF: usize = 64;
const TX_BUF: usize = 64;

// ---------------------------------------------------------------------------
// NTP task
// ---------------------------------------------------------------------------

/// NTP client task.
///
/// Waits for the network stack to have an IP address, then performs an initial
/// sync. Subsequently syncs once per hour. All results are stored in
/// [`UNIX_EPOCH_SECS`] and [`NTP_SYNCED`].
///
/// The task runs forever and does not return.
#[embassy_executor::task]
pub async fn ntp_task(stack: Stack<'static>) {
    stack.wait_config_up().await;
    info!("ntp: network up");

    info!("ntp: network ready, starting NTP sync");

    loop {
        match sync_once(stack).await {
            Some(unix_secs) => {
                UNIX_EPOCH_SECS.store(unix_secs, Ordering::Relaxed);
                NTP_SYNCED.store(true, Ordering::Relaxed);
                info!("ntp: synced — Unix epoch = {}", unix_secs);
            }
            None => {
                warn!("ntp: all {} attempts failed", MAX_ATTEMPTS);
            }
        }

        // Wait SYNC_INTERVAL_SECS before the next sync cycle
        Timer::after(Duration::from_secs(SYNC_INTERVAL_SECS)).await;
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Attempt a single NTP sync cycle with up to `MAX_ATTEMPTS` retries.
///
/// Returns the Unix epoch seconds on success, `None` if all attempts fail.
async fn sync_once(stack: Stack<'static>) -> Option<u32> {
    let mut rx_meta = [PacketMetadata::EMPTY; RX_META];
    let mut tx_meta = [PacketMetadata::EMPTY; TX_META];
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    // Bind to an ephemeral port (0 = let the stack choose)
    if socket.bind(0).is_err() {
        warn!("ntp: failed to bind UDP socket");
        return None;
    }

    let server_endpoint = IpEndpoint::new(NTP_SERVER_IP.into(), NTP_PORT);

    let mut req_buf = [0u8; 48];
    let req_len = encode_request(&mut req_buf);

    for attempt in 0..MAX_ATTEMPTS {
        // Send SNTP request
        if socket
            .send_to(&req_buf[..req_len], server_endpoint)
            .await
            .is_err()
        {
            warn!("ntp: send failed (attempt {})", attempt + 1);
            continue;
        }

        // Wait for a response with timeout
        let mut resp_buf = [0u8; 64];
        let recv_result = with_timeout(ATTEMPT_TIMEOUT, socket.recv_from(&mut resp_buf)).await;

        match recv_result {
            Ok(Ok((n, _meta))) => match decode_response(&resp_buf[..n]) {
                Ok(ts) => match ntp_to_unix_epoch(ts.seconds) {
                    Some(unix) => return Some(unix),
                    None => {
                        warn!("ntp: server returned pre-epoch timestamp {}", ts.seconds);
                    }
                },
                Err(_) => {
                    warn!("ntp: bad response (attempt {})", attempt + 1);
                }
            },
            Ok(Err(_)) => {
                warn!("ntp: recv error (attempt {})", attempt + 1);
            }
            Err(_timeout) => {
                warn!("ntp: timeout (attempt {})", attempt + 1);
            }
        }
    }

    None
}
