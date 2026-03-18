//! RFC 5424 syslog message formatter.
//!
//! Formats syslog messages suitable for transmission over UDP to a syslog
//! receiver. Implements the RFC 5424 SYSLOG-MSG format with a simplified
//! header (no structured data, nil MSG-ID, nil proc-ID).
//!
//! # Wire format
//! ```text
//! <PRI>1 TIMESTAMP HOSTNAME APP-NAME - - - MSG
//! ```
//! Where:
//! - `PRI` = facility * 8 + severity (decimal, in angle brackets)
//! - `1` = RFC 5424 version
//! - `TIMESTAMP` = ISO 8601 / RFC 3339 timestamp, or `-` if unknown
//! - `HOSTNAME` = device hostname or `-` if unknown
//! - `APP-NAME` = application name
//! - `-` `-` `-` = nil PROCID, nil MSGID, nil STRUCTURED-DATA
//! - `MSG` = free-form UTF-8 message
//!
//! This module is `no_std` and allocates nothing. All formatting is done into
//! a caller-supplied buffer. Long messages are silently truncated.

use crate::error::EncodeError;

// ---------------------------------------------------------------------------
// SyslogSeverity
// ---------------------------------------------------------------------------

/// RFC 5424 / RFC 3164 syslog severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SyslogSeverity {
    /// System is unusable.
    Emergency = 0,
    /// Action must be taken immediately.
    Alert = 1,
    /// Critical conditions.
    Critical = 2,
    /// Error conditions.
    Error = 3,
    /// Warning conditions.
    Warning = 4,
    /// Normal but significant condition.
    Notice = 5,
    /// Informational messages.
    Info = 6,
    /// Debug-level messages.
    Debug = 7,
}

// ---------------------------------------------------------------------------
// SyslogFacility
// ---------------------------------------------------------------------------

/// RFC 5424 / RFC 3164 syslog facility codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SyslogFacility {
    /// Kernel messages.
    Kern = 0,
    /// User-level messages.
    User = 1,
    /// Mail system.
    Mail = 2,
    /// System daemons.
    Daemon = 3,
    /// Security/authorization messages.
    Auth = 4,
    /// Messages generated internally by syslogd.
    Syslog = 5,
    /// Line printer subsystem.
    Lpr = 6,
    /// Network news subsystem.
    News = 7,
    /// UUCP subsystem.
    Uucp = 8,
    /// Clock daemon.
    Cron = 9,
    /// Security/authorization messages (private).
    AuthPriv = 10,
    /// FTP daemon.
    Ftp = 11,
    /// Reserved (NTP).
    Ntp = 12,
    /// Log audit.
    LogAudit = 13,
    /// Log alert.
    LogAlert = 14,
    /// Clock daemon (note 2).
    Clock = 15,
    /// Local use 0.
    Local0 = 16,
    /// Local use 1.
    Local1 = 17,
    /// Local use 2.
    Local2 = 18,
    /// Local use 3.
    Local3 = 19,
    /// Local use 4.
    Local4 = 20,
    /// Local use 5.
    Local5 = 21,
    /// Local use 6.
    Local6 = 22,
    /// Local use 7.
    Local7 = 23,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Format a syslog message into `buf` according to RFC 5424.
///
/// # Arguments
/// - `buf` — output buffer; the message is silently truncated if it would not
///   fit.  The buffer must be large enough for the header alone (typically
///   ~50 bytes); if the header itself doesn't fit, `Err(BufferTooSmall)` is
///   returned.
/// - `facility` — syslog facility code.
/// - `severity` — syslog severity level.
/// - `hostname` — device hostname (e.g. `"bacnet-bridge"`). Pass `"-"` if
///   unknown.
/// - `app_name` — application name (e.g. `"bacnet-bridge"`).
/// - `timestamp` — optional RFC 3339 timestamp string. If `None`, the nil
///   value `"-"` is used (appropriate when NTP is not yet synced).
/// - `msg` — the free-form log message. Truncated to fit in `buf`.
///
/// # Returns
/// The number of bytes written into `buf`, or `Err(EncodeError::BufferTooSmall)`
/// if `buf` is too small to hold even the header.
pub fn format_syslog(
    buf: &mut [u8],
    facility: SyslogFacility,
    severity: SyslogSeverity,
    hostname: &str,
    app_name: &str,
    timestamp: Option<&str>,
    msg: &str,
) -> Result<usize, EncodeError> {
    let pri = (facility as u8) * 8 + (severity as u8);
    let ts = timestamp.unwrap_or("-");

    // Format the fixed header: `<PRI>1 TIMESTAMP HOSTNAME APP-NAME - - - `
    // We use a heapless::String as a staging area for the fixed-width header
    // portion, then copy it plus the message into buf.
    //
    // Maximum header size (rough upper bound):
    //   <NNN> = 5 bytes
    //   "1 " = 2 bytes
    //   timestamp up to ~32 bytes + " " = 33 bytes
    //   hostname up to 255 bytes + " " = 256 bytes (RFC 5424 HOSTNAME ABNF)
    //   app_name up to 48 bytes + " " = 49 bytes
    //   "- - - " = 6 bytes
    //   Total: ~351 bytes — we use a 512-byte staging buffer.
    let mut hdr: heapless::Vec<u8, 512> = heapless::Vec::new();

    // <PRI>
    write_u8_decimal(&mut hdr, b'<')?;
    write_decimal_u8(&mut hdr, pri)?;
    write_u8_decimal(&mut hdr, b'>')?;

    // Version "1 "
    write_u8_decimal(&mut hdr, b'1')?;
    write_u8_decimal(&mut hdr, b' ')?;

    // TIMESTAMP
    write_str(&mut hdr, ts)?;
    write_u8_decimal(&mut hdr, b' ')?;

    // HOSTNAME
    write_str(&mut hdr, hostname)?;
    write_u8_decimal(&mut hdr, b' ')?;

    // APP-NAME
    write_str(&mut hdr, app_name)?;
    write_u8_decimal(&mut hdr, b' ')?;

    // PROCID MSGID STRUCTURED-DATA (all nil)
    write_str(&mut hdr, "- - - ")?;

    // Now copy header + message into buf, truncating if needed.
    let hdr_len = hdr.len();
    if buf.len() < hdr_len {
        return Err(EncodeError::BufferTooSmall);
    }
    buf[..hdr_len].copy_from_slice(hdr.as_slice());

    // Append as much of `msg` as fits
    let msg_bytes = msg.as_bytes();
    let remaining = buf.len() - hdr_len;
    let msg_len = msg_bytes.len().min(remaining);
    buf[hdr_len..hdr_len + msg_len].copy_from_slice(&msg_bytes[..msg_len]);

    Ok(hdr_len + msg_len)
}

/// Compute the PRI value: `facility * 8 + severity`.
#[inline]
pub fn syslog_pri(facility: SyslogFacility, severity: SyslogSeverity) -> u8 {
    (facility as u8) * 8 + (severity as u8)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write a single byte into a `heapless::Vec<u8, N>`.
#[inline]
fn write_u8_decimal<const N: usize>(
    v: &mut heapless::Vec<u8, N>,
    b: u8,
) -> Result<(), EncodeError> {
    v.push(b).map_err(|_| EncodeError::BufferTooSmall)
}

/// Write a `u8` value as decimal ASCII digits into `v`.
fn write_decimal_u8<const N: usize>(
    v: &mut heapless::Vec<u8, N>,
    val: u8,
) -> Result<(), EncodeError> {
    // u8 max = 255 — at most 3 digits
    if val >= 100 {
        write_u8_decimal(v, b'0' + val / 100)?;
        write_u8_decimal(v, b'0' + (val / 10) % 10)?;
        write_u8_decimal(v, b'0' + val % 10)?;
    } else if val >= 10 {
        write_u8_decimal(v, b'0' + val / 10)?;
        write_u8_decimal(v, b'0' + val % 10)?;
    } else {
        write_u8_decimal(v, b'0' + val)?;
    }
    Ok(())
}

/// Write a `&str` as bytes into `v`.
fn write_str<const N: usize>(v: &mut heapless::Vec<u8, N>, s: &str) -> Result<(), EncodeError> {
    for &b in s.as_bytes() {
        write_u8_decimal(v, b)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: format to a fixed-size array and return the message as a str.
    fn fmt(
        facility: SyslogFacility,
        severity: SyslogSeverity,
        hostname: &str,
        app_name: &str,
        ts: Option<&str>,
        msg: &str,
    ) -> heapless::String<1024> {
        let mut buf = [0u8; 1024];
        let n = format_syslog(&mut buf, facility, severity, hostname, app_name, ts, msg).unwrap();
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        let mut out: heapless::String<1024> = heapless::String::new();
        let _ = out.push_str(s);
        out
    }

    // ---- PRI calculation ---------------------------------------------------

    #[test]
    fn pri_calculation_local0_info() {
        // Local0 = 16, Info = 6 → PRI = 16*8 + 6 = 134
        assert_eq!(
            syslog_pri(SyslogFacility::Local0, SyslogSeverity::Info),
            134
        );
    }

    #[test]
    fn pri_calculation_kern_emergency() {
        // Kern = 0, Emergency = 0 → PRI = 0
        assert_eq!(
            syslog_pri(SyslogFacility::Kern, SyslogSeverity::Emergency),
            0
        );
    }

    #[test]
    fn pri_calculation_user_debug() {
        // User = 1, Debug = 7 → PRI = 1*8 + 7 = 15
        assert_eq!(syslog_pri(SyslogFacility::User, SyslogSeverity::Debug), 15);
    }

    #[test]
    fn pri_calculation_local7_debug() {
        // Local7 = 23, Debug = 7 → PRI = 23*8 + 7 = 191
        assert_eq!(
            syslog_pri(SyslogFacility::Local7, SyslogSeverity::Debug),
            191
        );
    }

    // ---- format_syslog header structure ------------------------------------

    #[test]
    fn format_begins_with_pri_in_angle_brackets() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "bacnet-bridge",
            "bacnet-bridge",
            None,
            "test",
        );
        // PRI = 134 → "<134>"
        assert!(
            s.as_str().starts_with("<134>"),
            "expected '<134>' prefix, got: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_version_is_1() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "h",
            "a",
            None,
            "m",
        );
        // After <134> comes "1 "
        assert!(
            s.as_str().contains(">1 "),
            "version must be 1: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_nil_timestamp_when_none() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "host",
            "app",
            None,
            "msg",
        );
        // Timestamp = "-"
        // Full header: <134>1 - host app - - - msg
        assert!(
            s.as_str().contains(">1 - "),
            "nil timestamp must be '-': {}",
            s.as_str()
        );
    }

    #[test]
    fn format_with_timestamp() {
        let ts = "2024-01-01T00:00:00Z";
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "host",
            "app",
            Some(ts),
            "msg",
        );
        assert!(
            s.as_str().contains(ts),
            "timestamp should appear verbatim: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_contains_hostname() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "bacnet-bridge",
            "bacnet-bridge",
            None,
            "hello",
        );
        assert!(
            s.as_str().contains("bacnet-bridge"),
            "hostname must appear: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_contains_app_name() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "host",
            "my-app",
            None,
            "hello",
        );
        assert!(
            s.as_str().contains("my-app"),
            "app_name must appear: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_contains_message() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Warning,
            "host",
            "app",
            None,
            "MS/TP token loss detected",
        );
        assert!(
            s.as_str().contains("MS/TP token loss detected"),
            "message must appear: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_nil_structured_data() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "h",
            "a",
            None,
            "m",
        );
        // Must contain "- - - " (three nil fields: PROCID, MSGID, SD)
        assert!(
            s.as_str().contains("- - - "),
            "must have three nil fields before MSG: {}",
            s.as_str()
        );
    }

    #[test]
    fn format_full_message_structure() {
        // Full expected format: <134>1 - bacnet-bridge bacnet-bridge - - - startup complete
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "bacnet-bridge",
            "bacnet-bridge",
            None,
            "startup complete",
        );
        let expected = "<134>1 - bacnet-bridge bacnet-bridge - - - startup complete";
        assert_eq!(s.as_str(), expected);
    }

    #[test]
    fn format_with_timestamp_full_structure() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Error,
            "bridge",
            "bacnet-bridge",
            Some("2024-06-01T12:00:00Z"),
            "watchdog fired",
        );
        // PRI = Local0(16)*8 + Error(3) = 131
        let expected = "<131>1 2024-06-01T12:00:00Z bridge bacnet-bridge - - - watchdog fired";
        assert_eq!(s.as_str(), expected);
    }

    // ---- truncation --------------------------------------------------------

    #[test]
    fn format_message_truncated_when_buf_too_small() {
        let mut buf = [0u8; 64];
        // Use a short app name and hostname so the header is predictably small
        let n = format_syslog(
            &mut buf,
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "h",
            "a",
            None,
            "This message is very long and should be truncated to fit the buffer",
        )
        .unwrap();
        assert!(n <= 64, "output must not exceed buffer size");
        // The header alone must be present
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.starts_with("<134>"));
    }

    #[test]
    fn format_buffer_too_small_for_header_returns_error() {
        let mut buf = [0u8; 4]; // too small even for "<0>1"
        let result = format_syslog(
            &mut buf,
            SyslogFacility::Kern,
            SyslogSeverity::Emergency,
            "h",
            "a",
            None,
            "msg",
        );
        assert_eq!(result.unwrap_err(), EncodeError::BufferTooSmall);
    }

    // ---- severity ordering -------------------------------------------------

    #[test]
    fn severity_ordering() {
        // Emergency is most severe (lowest priority value, highest urgency)
        assert!(SyslogSeverity::Emergency < SyslogSeverity::Alert);
        assert!(SyslogSeverity::Alert < SyslogSeverity::Critical);
        assert!(SyslogSeverity::Critical < SyslogSeverity::Error);
        assert!(SyslogSeverity::Error < SyslogSeverity::Warning);
        assert!(SyslogSeverity::Warning < SyslogSeverity::Notice);
        assert!(SyslogSeverity::Notice < SyslogSeverity::Info);
        assert!(SyslogSeverity::Info < SyslogSeverity::Debug);
    }

    // ---- facility values ---------------------------------------------------

    #[test]
    fn facility_values() {
        assert_eq!(SyslogFacility::Kern as u8, 0);
        assert_eq!(SyslogFacility::User as u8, 1);
        assert_eq!(SyslogFacility::Local0 as u8, 16);
        assert_eq!(SyslogFacility::Local7 as u8, 23);
    }

    // ---- edge cases --------------------------------------------------------

    #[test]
    fn format_empty_message() {
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "h",
            "a",
            None,
            "",
        );
        // Header should still be well-formed
        assert!(s.as_str().starts_with("<134>"));
        assert!(s.as_str().contains("- - - "));
    }

    #[test]
    fn format_nil_hostname() {
        // RFC 5424 allows "-" as the hostname when the FQDN is unknown
        let s = fmt(
            SyslogFacility::Local0,
            SyslogSeverity::Info,
            "-",
            "app",
            None,
            "msg",
        );
        assert!(s.as_str().contains(">1 - - app"), "got: {}", s.as_str());
    }

    #[test]
    fn pri_in_output_matches_syslog_pri_function() {
        let facility = SyslogFacility::Daemon;
        let severity = SyslogSeverity::Warning;
        let pri = syslog_pri(facility, severity);
        let mut buf = [0u8; 256];
        let n = format_syslog(&mut buf, facility, severity, "h", "a", None, "").unwrap();
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        // Build expected PRI string
        let mut expected_pri: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::write(&mut expected_pri, format_args!("<{}>", pri));
        assert!(s.starts_with(expected_pri.as_str()), "PRI mismatch: {}", s);
    }
}
