//! Syslog sender task and helpers.
//!
//! Sends RFC 5424 syslog messages over UDP to a configurable destination.
//! If no syslog target is configured ([`SYSLOG_TARGET`] is `None`), all calls
//! are no-ops.
//!
//! # Usage
//! ```rust,ignore
//! // One-shot: configure a target and log an event
//! syslog::set_target([192, 168, 1, 100], 514);
//! syslog::send_log(stack, SyslogSeverity::Info, "startup complete").await;
//! ```
//!
//! # Thread safety
//! [`SYSLOG_TARGET`] is protected by an `embassy_sync::blocking_mutex::Mutex`
//! (critical-section based), making it safe to access from any async task.
//! `send_log` opens a fresh UDP socket per call — intentionally simple at the
//! cost of socket overhead. Syslog is low-frequency, so this is acceptable.

use bridge_core::syslog::{format_syslog, SyslogFacility, SyslogSeverity};
use defmt::warn;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpEndpoint, Ipv4Address, Stack};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

// ---------------------------------------------------------------------------
// Shared syslog target
// ---------------------------------------------------------------------------

/// The current syslog destination (IP, port).
///
/// When `None`, syslog sending is disabled. Set via [`set_target`] or
/// [`clear_target`].
pub static SYSLOG_TARGET: Mutex<CriticalSectionRawMutex, Option<([u8; 4], u16)>> = Mutex::new(None);

/// Configure the syslog destination.
///
/// Call this after loading config from flash or when the admin UI updates
/// the syslog server address.
pub async fn set_target(ip: [u8; 4], port: u16) {
    let mut t = SYSLOG_TARGET.lock().await;
    *t = Some((ip, port));
}

/// Disable syslog (clear the target).
pub async fn clear_target() {
    let mut t = SYSLOG_TARGET.lock().await;
    *t = None;
}

// ---------------------------------------------------------------------------
// Public logging API
// ---------------------------------------------------------------------------

/// Send a syslog message with the specified severity.
///
/// If [`SYSLOG_TARGET`] is `None` this is a no-op. On UDP send failure the
/// error is silently dropped — syslog is best-effort.
///
/// The device hostname is read from the HTTP config store; if not yet
/// available, `"bacnet-bridge"` is used as a fallback.
pub async fn send_log(stack: Stack<'static>, severity: SyslogSeverity, msg: &str) {
    // Check target under lock, then release before awaiting the socket
    let target = {
        let t = SYSLOG_TARGET.lock().await;
        *t
    };

    let (dest_ip, dest_port) = match target {
        Some(t) => t,
        None => return, // syslog disabled
    };

    // Read hostname from config (non-blocking; use fallback on contention)
    let hostname = {
        let guard = crate::http::CONFIG.lock().await;
        match guard.as_ref() {
            Some(cfg) => {
                let mut h: heapless::String<32> = heapless::String::new();
                let _ = h.push_str(cfg.hostname.as_str());
                h
            }
            None => {
                let mut h: heapless::String<32> = heapless::String::new();
                let _ = h.push_str("bacnet-bridge");
                h
            }
        }
    };

    // Format the syslog message into a stack buffer
    let mut buf = [0u8; 512];
    let n = match format_syslog(
        &mut buf,
        SyslogFacility::Local0,
        severity,
        hostname.as_str(),
        "bacnet-bridge",
        None, // no timestamp until NTP syncs
        msg,
    ) {
        Ok(n) => n,
        Err(_) => {
            warn!("syslog: format failed");
            return;
        }
    };

    // Open a UDP socket, send, close
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 64];
    let mut tx_buf = [0u8; 512];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    if socket.bind(0).is_err() {
        warn!("syslog: bind failed");
        return;
    }

    let endpoint = IpEndpoint::new(
        Ipv4Address::new(dest_ip[0], dest_ip[1], dest_ip[2], dest_ip[3]).into(),
        dest_port,
    );

    if let Err(_) = socket.send_to(&buf[..n], endpoint).await {
        warn!("syslog: send_to failed");
    }
}

// ---------------------------------------------------------------------------
// Convenience wrappers
// ---------------------------------------------------------------------------

/// Log an informational message.
pub async fn info(stack: Stack<'static>, msg: &str) {
    send_log(stack, SyslogSeverity::Info, msg).await;
}

/// Log a warning message.
pub async fn warning(stack: Stack<'static>, msg: &str) {
    send_log(stack, SyslogSeverity::Warning, msg).await;
}

/// Log an error message.
pub async fn error(stack: Stack<'static>, msg: &str) {
    send_log(stack, SyslogSeverity::Error, msg).await;
}

/// Log a debug message.
pub async fn debug(stack: Stack<'static>, msg: &str) {
    send_log(stack, SyslogSeverity::Debug, msg).await;
}
