//! Server-Sent Events (SSE) support for live BACnet point value updates.
//!
//! `handle_sse` keeps the TCP connection open and streams JSON events at
//! a fixed 1-second interval for any point that has a dirty (changed) value.

use crate::bridge::BRIDGE_STATE;
use defmt::warn;
use embassy_net::tcp::TcpSocket;
use embassy_time::Timer;
use embedded_io_async::Write;

/// SSE response headers written at connection start.
const SSE_HEADERS: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: keep-alive\r\n\
Access-Control-Allow-Origin: *\r\n\
\r\n";

/// Poll interval for SSE value updates.
const POLL_INTERVAL_MS: u64 = 1000;

/// Handle a single SSE connection.
///
/// Sends the SSE headers then loops, emitting `data: {...}\n\n` JSON lines
/// for each point whose value has changed since the last poll.
pub async fn handle_sse(socket: &mut TcpSocket<'_>) {
    // Write SSE headers
    if socket.write_all(SSE_HEADERS).await.is_err() {
        return;
    }
    if socket.flush().await.is_err() {
        return;
    }

    loop {
        Timer::after_millis(POLL_INTERVAL_MS).await;

        // Collect dirty points and clear the dirty flag
        let mut events: heapless::Vec<heapless::String<256>, 16> = heapless::Vec::new();

        {
            let mut state = BRIDGE_STATE.lock().await;
            for dev_idx in 0..state.device_count {
                let device_id = state.devices[dev_idx].device_id;
                let count = state.point_counts[dev_idx];
                for pt_idx in 0..count {
                    if let Some(ref mut point) = state.points[dev_idx][pt_idx] {
                        if !point.dirty {
                            continue;
                        }
                        point.dirty = false;

                        // Build a minimal JSON event string
                        // {"deviceId":N,"objType":N,"instance":N,"value":...}
                        let mut event: heapless::String<256> = heapless::String::new();
                        let obj_type_code = point.object_id.object_type.code();
                        let instance = point.object_id.instance;

                        // Format the value part
                        let value_str: heapless::String<64> = match &point.present_value {
                            Some(bridge_core::bacnet::BacnetValue::Real(f)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                // Format float manually — no std::fmt float support in no_std
                                // Use integer * 1000 representation
                                let whole = *f as i32;
                                let frac = ((*f - whole as f32).abs() * 1000.0) as u32;
                                let _ = core::fmt::write(
                                    &mut s,
                                    format_args!("{}.{:03}", whole, frac),
                                );
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::UnsignedInt(n)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = core::fmt::write(&mut s, format_args!("{}", n));
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::Boolean(b)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = core::fmt::write(&mut s, format_args!("{}", b));
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::SignedInt(n)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = core::fmt::write(&mut s, format_args!("{}", n));
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::Enumerated(n)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = core::fmt::write(&mut s, format_args!("{}", n));
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::Null) | None => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = s.push_str("null");
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::CharString(cs)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = s.push('"');
                                let _ = s.push_str(cs.as_str());
                                let _ = s.push('"');
                                s
                            }
                            Some(bridge_core::bacnet::BacnetValue::ObjectIdentifier(oid)) => {
                                let mut s: heapless::String<64> = heapless::String::new();
                                let _ = core::fmt::write(
                                    &mut s,
                                    format_args!(
                                        "\"{}:{}\"",
                                        oid.object_type.code(),
                                        oid.instance
                                    ),
                                );
                                s
                            }
                        };

                        let _ = core::fmt::write(
                            &mut event,
                            format_args!(
                                "{{\"deviceId\":{},\"objType\":{},\"instance\":{},\"value\":{}}}",
                                device_id, obj_type_code, instance, value_str.as_str()
                            ),
                        );

                        let _ = events.push(event);
                        if events.is_full() {
                            break;
                        }
                    }
                }
                if events.is_full() {
                    break;
                }
            }
        } // mutex released

        // Send the collected events
        for event in &events {
            let mut line: heapless::Vec<u8, 320> = heapless::Vec::new();
            let _ = line.extend_from_slice(b"data: ");
            let _ = line.extend_from_slice(event.as_bytes());
            let _ = line.extend_from_slice(b"\n\n");
            if socket.write_all(&line).await.is_err() {
                warn!("sse: write error, closing");
                return;
            }
        }

        // Send a keep-alive comment every cycle
        if events.is_empty() {
            if socket.write_all(b": ping\n\n").await.is_err() {
                return;
            }
        }

        if socket.flush().await.is_err() {
            return;
        }
    }
}
