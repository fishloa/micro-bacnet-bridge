//! MQTT 3.1.1 publish-only client task.
//!
//! Connects to a configured MQTT broker and:
//! 1. Publishes Home Assistant auto-discovery payloads (with `retain=true`)
//!    for all known BACnet points when `ha_discovery_enabled` is set in config.
//! 2. Polls the bridge state every second for dirty points and publishes
//!    changed values to `{topic_prefix}/{object_type}/{instance}/state`.
//! 3. Sends PINGREQ every `keep_alive / 2` seconds to keep the connection alive.
//! 4. On disconnect, waits 10 s then reconnects.
//!
//! If MQTT is not enabled in config the task returns immediately.
//!
//! # TLS support
//! When `config.mqtt.tls_enabled` is true the TCP socket is wrapped with
//! `embedded-tls` (`TlsConnection`) before the MQTT CONNECT packet is sent.
//! Certificate verification is intentionally skipped (`UnsecureProvider`) for
//! now — a CA cert upload path is a future TODO.  The TLS buffers (4 KB rx +
//! 4 KB tx) live on the stack; RP2350's 520 KB SRAM makes this comfortable.
//!
//! The MQTT read/write logic is shared between plain-TCP and TLS sessions via
//! the `mqtt_session` helper which is generic over `embedded_io_async::Read +
//! Write`.
//!
//! # Socket usage
//! Uses a single TCP socket for the broker connection. DNS resolution uses the
//! `dns::resolve` helper which opens/closes its own UDP socket per call.

use bridge_core::bacnet::BacnetValue;
use bridge_core::mqtt::{
    decode_connack, encode_connect, encode_disconnect, encode_pingreq, encode_publish,
    format_ha_discovery, ha_discovery_topic, HaDiscoveryParams, MQTT_PORT,
};
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_rp::clocks::RoscRng;
use embassy_time::{with_timeout, Duration, Timer};
use embedded_io_async::{Read as AsyncRead, Write as AsyncWrite};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
use heapless::String;

// ---------------------------------------------------------------------------
// Configuration constants (replace with config fields in a future revision)
// ---------------------------------------------------------------------------

/// Default MQTT broker hostname.
const DEFAULT_BROKER_HOST: &str = "mqtt.local";

/// Default MQTT broker port.
const DEFAULT_BROKER_PORT: u16 = MQTT_PORT;

/// MQTT keep-alive interval (seconds). PINGREQ is sent every `KEEP_ALIVE / 2`.
const KEEP_ALIVE_SECS: u16 = 60;

/// MQTT client identifier.
const CLIENT_ID: &str = "bacnet-bridge";

/// Topic prefix: `{prefix}/{object_type}/{instance}/state`.
const TOPIC_PREFIX: &str = "bacnet-bridge";

/// Home Assistant discovery prefix.
const HA_DISCOVERY_PREFIX: &str = "homeassistant";

/// Whether to publish HA discovery payloads on connect.
const HA_DISCOVERY_ENABLED: bool = true;

/// Interval between dirty-point polls (milliseconds).
const POLL_INTERVAL_MS: u64 = 1000;

/// Reconnect delay after a disconnect or connection failure.
const RECONNECT_DELAY: Duration = Duration::from_secs(10);

/// TCP socket buffer sizes.
const RX_BUF: usize = 1024;
const TX_BUF: usize = 1024;

/// Connect-response timeout.
const CONNACK_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// MQTT client task
// ---------------------------------------------------------------------------

/// MQTT client task.
///
/// Checks whether MQTT is configured (based on whether the broker hostname is
/// the default or a real hostname can be resolved), then enters a connect →
/// publish loop. Returns immediately if MQTT should be disabled.
#[embassy_executor::task]
pub async fn mqtt_task(stack: Stack<'static>) {
    stack.wait_config_up().await;
    info!("mqtt: network up");

    // Check if MQTT is enabled in config
    let (enabled, tls_enabled) = {
        let cfg = crate::http::CONFIG.lock().await;
        match cfg.as_ref() {
            Some(c) => (c.mqtt.enabled, c.mqtt.tls_enabled),
            None => (false, false),
        }
    };

    if !enabled {
        info!("mqtt: disabled in config, task idle");
        // Sleep forever — don't burn CPU or make DNS queries
        loop {
            Timer::after(embassy_time::Duration::from_secs(3600)).await;
        }
    }

    info!("mqtt: network ready, starting MQTT client");

    loop {
        // Resolve broker IP
        let broker_ip = match crate::dns::resolve(stack, DEFAULT_BROKER_HOST).await {
            Some(ip) => ip,
            None => {
                warn!("mqtt: broker DNS resolution failed; retry in 10 s");
                Timer::after(RECONNECT_DELAY).await;
                continue;
            }
        };

        info!(
            "mqtt: broker {}:{} resolved (tls={})",
            DEFAULT_BROKER_HOST, DEFAULT_BROKER_PORT, tls_enabled
        );

        // Attempt connection and run session
        run_session(stack, broker_ip, DEFAULT_BROKER_PORT, tls_enabled).await;

        // Brief pause before reconnecting
        warn!("mqtt: disconnected; reconnecting in 10 s");
        Timer::after(RECONNECT_DELAY).await;
    }
}

// ---------------------------------------------------------------------------
// Session runner
// ---------------------------------------------------------------------------

/// Connect to the broker, optionally wrap with TLS, then run the MQTT session.
///
/// Returns when the connection drops or a fatal error occurs.
async fn run_session(stack: Stack<'static>, broker_ip: [u8; 4], port: u16, tls_enabled: bool) {
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(30)));

    let remote = embassy_net::IpEndpoint::new(
        embassy_net::Ipv4Address::new(broker_ip[0], broker_ip[1], broker_ip[2], broker_ip[3])
            .into(),
        port,
    );

    if socket.connect(remote).await.is_err() {
        warn!("mqtt: TCP connect failed");
        return;
    }

    info!("mqtt: TCP connected");

    if tls_enabled {
        // TLS path: wrap the TCP socket with embedded-tls.
        // 4 KB rx + 4 KB tx TLS record buffers on the stack.
        let mut tls_read_buf = [0u8; 4096];
        let mut tls_write_buf = [0u8; 4096];

        // TlsConfig has no cipher-suite generic; the cipher suite is chosen
        // at open() time via the CryptoProvider's associated type.
        let tls_config = TlsConfig::new();

        // UnsecureProvider skips certificate verification.
        // RoscRng is the RP2350 hardware RNG, which implements CryptoRngCore.
        // TODO: add CA cert upload and switch to a verifying provider.
        let rng = RoscRng;
        let tls_ctx = TlsContext::new(&tls_config, UnsecureProvider::new::<Aes128GcmSha256>(rng));

        let mut tls: TlsConnection<'_, TcpSocket<'_>, Aes128GcmSha256> =
            TlsConnection::new(socket, &mut tls_read_buf, &mut tls_write_buf);

        match tls.open(tls_ctx).await {
            Ok(()) => info!("mqtt: TLS handshake complete"),
            Err(_) => {
                warn!("mqtt: TLS handshake failed");
                return;
            }
        }

        mqtt_session(&mut tls).await;
    } else {
        // Plain TCP path.
        mqtt_session(&mut socket).await;
        socket.close();
    }
}

/// Run the MQTT CONNECT → publish loop over any `Read + Write` transport.
///
/// This function is generic so that the same logic works for both plain TCP
/// (`TcpSocket`) and TLS (`TlsConnection`).
async fn mqtt_session(transport: &mut (impl AsyncRead + AsyncWrite)) {
    // Send MQTT CONNECT
    let mut pkt_buf = [0u8; 128];
    let n = match encode_connect(&mut pkt_buf, CLIENT_ID, KEEP_ALIVE_SECS, None, None) {
        Ok(n) => n,
        Err(_) => {
            warn!("mqtt: failed to encode CONNECT");
            return;
        }
    };

    if transport.write_all(&pkt_buf[..n]).await.is_err() {
        warn!("mqtt: failed to send CONNECT");
        return;
    }

    // Wait for CONNACK
    let mut resp_buf = [0u8; 16];
    let connack_result = with_timeout(
        CONNACK_TIMEOUT,
        read_at_least_generic(transport, &mut resp_buf, 4),
    )
    .await;

    match connack_result {
        Ok(Ok(n)) => match decode_connack(&resp_buf[..n]) {
            Ok(true) => info!("mqtt: CONNACK accepted"),
            Ok(false) => {
                warn!("mqtt: broker rejected connection");
                return;
            }
            Err(_) => {
                warn!("mqtt: malformed CONNACK");
                return;
            }
        },
        _ => {
            warn!("mqtt: CONNACK timeout or recv error");
            return;
        }
    }

    // Publish HA discovery messages for all known points
    if HA_DISCOVERY_ENABLED {
        publish_ha_discovery_generic(transport).await;
    }

    // Main publish loop
    let mut ticks_since_ping: u64 = 0;
    let ping_interval_ticks = (KEEP_ALIVE_SECS as u64 / 2).max(1) * (1000 / POLL_INTERVAL_MS);

    loop {
        // Check for dirty points and publish
        if publish_dirty_points_generic(transport).await.is_err() {
            warn!("mqtt: publish failed; dropping connection");
            break;
        }

        // Send PINGREQ periodically
        ticks_since_ping += 1;
        if ticks_since_ping >= ping_interval_ticks {
            ticks_since_ping = 0;
            let mut ping_buf = [0u8; 2];
            if let Ok(n) = encode_pingreq(&mut ping_buf) {
                if transport.write_all(&ping_buf[..n]).await.is_err() {
                    warn!("mqtt: PINGREQ failed; dropping connection");
                    break;
                }
            }
        }

        Timer::after_millis(POLL_INTERVAL_MS).await;
    }

    // Attempt clean disconnect
    let mut disc_buf = [0u8; 2];
    if let Ok(n) = encode_disconnect(&mut disc_buf) {
        let _ = transport.write_all(&disc_buf[..n]).await;
    }
}

// ---------------------------------------------------------------------------
// HA discovery publisher
// ---------------------------------------------------------------------------

/// Publish Home Assistant auto-discovery payloads for all currently known
/// points in the bridge state.
///
/// Generic over any `AsyncWrite` transport (plain TCP or TLS).
async fn publish_ha_discovery_generic(writer: &mut impl AsyncWrite) {
    let state = crate::bridge::BRIDGE_STATE.lock().await;
    let device_count = state.device_count;
    drop(state); // release lock before doing network I/O

    for dev_idx in 0..device_count {
        // Snapshot device + points under lock
        let (dev_id, dev_name, point_count) = {
            let state = crate::bridge::BRIDGE_STATE.lock().await;
            let dev = &state.devices[dev_idx];
            let pc = state.point_counts[dev_idx];
            (dev.device_id, dev.name.clone(), pc)
        };

        if dev_id == 0 {
            continue;
        }

        for pt_idx in 0..point_count {
            // Snapshot a single point
            let point = {
                let state = crate::bridge::BRIDGE_STATE.lock().await;
                state.points[dev_idx][pt_idx].clone()
            };
            let pt = match point {
                Some(p) => p,
                None => continue,
            };

            let object_type_str = object_type_str(pt.object_id.object_type);
            let instance = pt.object_id.instance;
            let point_name = if pt.name.is_empty() {
                pt.object_id.object_type.to_str()
            } else {
                pt.name.as_str()
            };

            // Build state topic
            let mut state_topic: String<128> = String::new();
            if build_state_topic(&mut state_topic, TOPIC_PREFIX, object_type_str, instance).is_err()
            {
                continue;
            }

            // Build discovery topic
            let device_name_str = if dev_name.is_empty() {
                CLIENT_ID
            } else {
                dev_name.as_str()
            };
            let mut disc_topic: String<128> = String::new();
            if ha_discovery_topic(
                &mut disc_topic,
                HA_DISCOVERY_PREFIX,
                device_name_str,
                object_type_str,
                instance,
            )
            .is_err()
            {
                continue;
            }

            // Build discovery payload
            let mut payload_buf = [0u8; 512];
            let payload_len = match format_ha_discovery(
                &mut payload_buf,
                &HaDiscoveryParams {
                    discovery_prefix: HA_DISCOVERY_PREFIX,
                    device_name: device_name_str,
                    point_name,
                    object_type: object_type_str,
                    object_instance: instance,
                    unit: "",
                    state_topic: state_topic.as_str(),
                },
            ) {
                Ok(n) => n,
                Err(_) => continue,
            };

            // Encode and send PUBLISH (retain=true)
            let mut pkt_buf = [0u8; 700];
            if let Ok(n) = encode_publish(
                &mut pkt_buf,
                disc_topic.as_str(),
                &payload_buf[..payload_len],
                true,
            ) {
                if writer.write_all(&pkt_buf[..n]).await.is_err() {
                    return; // transport dead
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dirty-point publisher
// ---------------------------------------------------------------------------

/// Scan bridge state for dirty points, publish changed values, clear dirty flag.
///
/// Generic over any `AsyncWrite` transport (plain TCP or TLS).
/// Returns `Ok(())` on success, `Err(())` if the transport write fails.
async fn publish_dirty_points_generic(writer: &mut impl AsyncWrite) -> Result<(), ()> {
    // Collect dirty points without holding the lock across network I/O
    // (heapless::Vec limits to MAX_DEVICES * MAX_POINTS = 8 * 32 = 256 entries)
    // We iterate one device at a time to avoid large stack allocations.
    let device_count = crate::bridge::BRIDGE_STATE.lock().await.device_count;

    for dev_idx in 0..device_count {
        let point_count = crate::bridge::BRIDGE_STATE.lock().await.point_counts[dev_idx];

        for pt_idx in 0..point_count {
            // Read point under lock
            let (object_type, instance, value, dirty) = {
                let state = crate::bridge::BRIDGE_STATE.lock().await;
                match &state.points[dev_idx][pt_idx] {
                    Some(p) => {
                        let v = p.present_value.clone();
                        (p.object_id.object_type, p.object_id.instance, v, p.dirty)
                    }
                    None => continue,
                }
            };

            if !dirty {
                continue;
            }

            // Clear dirty flag under lock
            {
                let mut state = crate::bridge::BRIDGE_STATE.lock().await;
                if let Some(p) = &mut state.points[dev_idx][pt_idx] {
                    p.dirty = false;
                }
            }

            // Format the value as a string
            let value_str = format_value(value.as_ref());

            // Build topic
            let mut topic: String<128> = String::new();
            let object_type_str = object_type_str(object_type);
            if build_state_topic(&mut topic, TOPIC_PREFIX, object_type_str, instance).is_err() {
                continue;
            }

            // Encode PUBLISH (QoS 0, no retain)
            let mut pkt_buf = [0u8; 256];
            if let Ok(n) = encode_publish(&mut pkt_buf, topic.as_str(), value_str.as_bytes(), false)
            {
                writer.write_all(&pkt_buf[..n]).await.map_err(|_| ())?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a state topic: `{prefix}/{object_type}/{instance}/state`.
fn build_state_topic(
    out: &mut String<128>,
    prefix: &str,
    object_type: &str,
    instance: u32,
) -> Result<(), ()> {
    use core::fmt::Write;
    write!(out, "{}/{}/{}/state", prefix, object_type, instance).map_err(|_| ())
}

/// Format a `BacnetValue` as a compact string for MQTT publishing.
///
/// Returns a fixed-size `heapless::String<32>` with the formatted value.
fn format_value(value: Option<&BacnetValue>) -> String<32> {
    use core::fmt::Write;
    let mut s: String<32> = String::new();
    match value {
        None => {
            let _ = s.push_str("null");
        }
        Some(BacnetValue::Null) => {
            let _ = s.push_str("null");
        }
        Some(BacnetValue::Boolean(b)) => {
            let _ = s.push_str(if *b { "true" } else { "false" });
        }
        Some(BacnetValue::UnsignedInt(v)) => {
            let _ = write!(s, "{}", v);
        }
        Some(BacnetValue::SignedInt(v)) => {
            let _ = write!(s, "{}", v);
        }
        Some(BacnetValue::Real(f)) => {
            // Format as fixed-point with 2 decimal places (no std fmt)
            // Multiply by 100, round, then format as integer parts.
            let int_part = *f as i32;
            let frac_abs = ((*f - int_part as f32) * 100.0).abs() as u32;
            let _ = write!(s, "{}.{:02}", int_part, frac_abs);
        }
        Some(BacnetValue::CharString(cs)) => {
            // Truncate to 32 chars
            let bytes = cs.as_bytes();
            let n = bytes.len().min(31);
            for &b in &bytes[..n] {
                if s.push(b as char).is_err() {
                    break;
                }
            }
        }
        Some(BacnetValue::Enumerated(v)) => {
            let _ = write!(s, "{}", v);
        }
        Some(BacnetValue::ObjectIdentifier(oid)) => {
            let _ = write!(
                s,
                "{}:{}",
                bridge_core::bacnet::ObjectType::code(oid.object_type),
                oid.instance
            );
        }
    }
    s
}

/// Return a short lowercase string representation of an `ObjectType`.
fn object_type_str(ot: bridge_core::bacnet::ObjectType) -> &'static str {
    use bridge_core::bacnet::ObjectType;
    match ot {
        ObjectType::AnalogInput => "analog-input",
        ObjectType::AnalogOutput => "analog-output",
        ObjectType::AnalogValue => "analog-value",
        ObjectType::BinaryInput => "binary-input",
        ObjectType::BinaryOutput => "binary-output",
        ObjectType::BinaryValue => "binary-value",
        ObjectType::Calendar => "calendar",
        ObjectType::Device => "device",
        ObjectType::MultiStateInput => "multi-state-input",
        ObjectType::MultiStateOutput => "multi-state-output",
        ObjectType::NotificationClass => "notification-class",
        ObjectType::Schedule => "schedule",
        ObjectType::MultiStateValue => "multi-state-value",
        ObjectType::TrendLog => "trend-log",
    }
}

// ---------------------------------------------------------------------------
// Low-level I/O helpers
// ---------------------------------------------------------------------------

/// Read at least `min_len` bytes into `buf` from any `AsyncRead` transport.
///
/// Returns the number of bytes read (which may be more than `min_len` if the
/// transport delivered a larger chunk), or `Err(())` on error / closure.
async fn read_at_least_generic(
    reader: &mut impl AsyncRead,
    buf: &mut [u8],
    min_len: usize,
) -> Result<usize, ()> {
    let mut total = 0;
    while total < min_len {
        let n = reader.read(&mut buf[total..]).await.map_err(|_| ())?;
        if n == 0 {
            return Err(()); // connection closed
        }
        total += n;
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// ObjectType display helper (needed for point_name fallback)
// ---------------------------------------------------------------------------

trait ObjectTypeExt {
    fn to_str(self) -> &'static str;
}

impl ObjectTypeExt for bridge_core::bacnet::ObjectType {
    fn to_str(self) -> &'static str {
        object_type_str(self)
    }
}
