//! Minimal HTTP/1.1 server for the BACnet bridge admin interface.
//!
//! Runs on TCP port 80. Routes:
//! - Static SvelteKit assets (gzip, from `web_assets`)
//! - REST API endpoints under `/api/v1/`
//! - SSE stream at `/api/events`

use crate::bridge::BRIDGE_STATE;
use crate::sse::handle_sse;
use crate::web_assets;
use bridge_core::config::BridgeConfig;
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_io_async::Write;

/// TCP port for the HTTP server.
const HTTP_PORT: u16 = 80;

/// Receive buffer per connection.
const RX_BUF: usize = 2048;
/// Transmit buffer per connection.
const TX_BUF: usize = 4096;
/// Maximum request line length we bother parsing.
const REQ_BUF: usize = 512;

/// Global reference to the current bridge config.
/// Initialised to `None`; main sets it before spawning tasks.
/// After init, always `Some`.
pub static CONFIG: Mutex<CriticalSectionRawMutex, Option<BridgeConfig>> = Mutex::new(None);

/// Main HTTP server task. Accepts one connection at a time.
#[embassy_executor::task]
pub async fn http_task(stack: Stack<'static>) {
    let mut rx_buf = [0u8; RX_BUF];
    let mut tx_buf = [0u8; TX_BUF];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(embassy_time::Duration::from_secs(10)));

        info!("http: waiting for connection on port {}", HTTP_PORT);
        if socket.accept(HTTP_PORT).await.is_err() {
            warn!("http: accept error");
            Timer::after_millis(100).await;
            continue;
        }

        info!("http: connection accepted");
        handle_connection(&mut socket).await;
        socket.close();
        // Small yield to allow the stack to process the close
        Timer::after_millis(10).await;
    }
}

/// Handle a single HTTP connection (one request/response cycle).
async fn handle_connection(socket: &mut TcpSocket<'_>) {
    let mut req_buf = [0u8; REQ_BUF];
    let n = match read_request(socket, &mut req_buf).await {
        Some(n) => n,
        None => {
            send_400(socket).await;
            return;
        }
    };

    let request = match core::str::from_utf8(&req_buf[..n]) {
        Ok(s) => s,
        Err(_) => {
            send_400(socket).await;
            return;
        }
    };

    // Parse request line: "METHOD /path HTTP/1.1"
    let mut lines = request.splitn(3, "\r\n");
    let request_line = match lines.next() {
        Some(l) => l,
        None => {
            send_400(socket).await;
            return;
        }
    };

    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    info!("http: {} {}", method, path);

    match (method, path) {
        // ---- Static assets ----
        ("GET", "/") | ("GET", "/index.html") => serve_asset(socket, "/index.html").await,
        ("GET", p) if p.starts_with("/_app/") => serve_asset(socket, p).await,
        ("GET", "/robots.txt") => serve_asset(socket, "/robots.txt").await,

        // ---- SSE ----
        ("GET", "/api/events") => handle_sse(socket).await,

        // ---- REST API ----
        ("GET", "/api/v1/devices") => api_get_devices(socket).await,
        ("GET", "/api/v1/config/network") => api_get_network_config(socket).await,
        ("GET", "/api/v1/config/bacnet") => api_get_bacnet_config(socket).await,
        ("GET", "/api/v1/system/status") => api_get_status(socket).await,

        ("PUT", "/api/v1/config/network") => {
            let body = read_body(request);
            api_put_network_config(socket, body).await;
        }
        ("PUT", "/api/v1/config/bacnet") => {
            let body = read_body(request);
            api_put_bacnet_config(socket, body).await;
        }

        // ---- OpenAPI spec ----
        ("GET", "/api/openapi.json") => send_static_json(socket, OPENAPI_STUB).await,

        // ---- Fallthrough ----
        _ => send_404(socket).await,
    }
}

// ---------------------------------------------------------------------------
// Request parsing helpers
// ---------------------------------------------------------------------------

/// Read bytes from the socket until we see the end of the HTTP header block
/// (`\r\n\r\n`) or the buffer is full. Returns the number of bytes read.
async fn read_request(socket: &mut TcpSocket<'_>, buf: &mut [u8]) -> Option<usize> {
    let mut total = 0usize;
    loop {
        if total >= buf.len() {
            return None;
        }
        match socket.read(&mut buf[total..]).await {
            Ok(0) => return None,
            Ok(n) => {
                total += n;
                // Check if we have a complete header block
                if contains_header_end(&buf[..total]) {
                    return Some(total);
                }
            }
            Err(_) => return None,
        }
    }
}

fn contains_header_end(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// Extract the body part (after \r\n\r\n) from a request string.
fn read_body(request: &str) -> &str {
    if let Some(pos) = request.find("\r\n\r\n") {
        &request[pos + 4..]
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Static asset serving
// ---------------------------------------------------------------------------

async fn serve_asset(socket: &mut TcpSocket<'_>, path: &str) {
    match web_assets::get_asset(path) {
        Some((data, content_type)) => {
            let mut hdr: heapless::Vec<u8, 256> = heapless::Vec::new();
            let _ = hdr.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
            let _ = hdr.extend_from_slice(b"Content-Encoding: gzip\r\n");
            let _ = hdr.extend_from_slice(b"Content-Type: ");
            let _ = hdr.extend_from_slice(content_type.as_bytes());
            let _ = hdr.extend_from_slice(b"\r\n");
            // Cache control: cache immutable assets, revalidate others
            if path.contains("immutable") {
                let _ = hdr
                    .extend_from_slice(b"Cache-Control: public, max-age=31536000, immutable\r\n");
            } else {
                let _ = hdr.extend_from_slice(b"Cache-Control: no-cache\r\n");
            }
            // Write content-length
            let mut len_line: heapless::String<32> = heapless::String::new();
            let _ = core::fmt::write(
                &mut len_line,
                format_args!("Content-Length: {}\r\n\r\n", data.len()),
            );
            let _ = hdr.extend_from_slice(len_line.as_bytes());

            if socket.write_all(&hdr).await.is_err() {
                return;
            }
            // Stream the asset in chunks to avoid large stack buffers
            let chunk_size = 512;
            let mut offset = 0;
            while offset < data.len() {
                let end = (offset + chunk_size).min(data.len());
                if socket.write_all(&data[offset..end]).await.is_err() {
                    return;
                }
                offset = end;
            }
            let _ = socket.flush().await;
        }
        None => send_404(socket).await,
    }
}

// ---------------------------------------------------------------------------
// API handlers
// ---------------------------------------------------------------------------

async fn api_get_devices(socket: &mut TcpSocket<'_>) {
    let state = BRIDGE_STATE.lock().await;
    let mut body: heapless::Vec<u8, 1024> = heapless::Vec::new();
    let _ = body.extend_from_slice(b"[");
    let mut first = true;
    for i in 0..state.device_count {
        let d = &state.devices[i];
        if !first {
            let _ = body.extend_from_slice(b",");
        }
        first = false;
        let mut entry: heapless::String<128> = heapless::String::new();
        let _ = core::fmt::write(
            &mut entry,
            format_args!(
                "{{\"deviceId\":{},\"name\":\"{}\",\"pointsLoaded\":{}}}",
                d.device_id,
                d.name.as_str(),
                d.points_loaded
            ),
        );
        let _ = body.extend_from_slice(entry.as_bytes());
    }
    let _ = body.extend_from_slice(b"]");
    send_json(socket, &body).await;
}

async fn api_get_network_config(socket: &mut TcpSocket<'_>) {
    let guard = CONFIG.lock().await;
    let cfg = match guard.as_ref() {
        Some(c) => c,
        None => {
            send_404(socket).await;
            return;
        }
    };
    let net = &cfg.network;
    let mut body: heapless::String<256> = heapless::String::new();
    let _ = core::fmt::write(
        &mut body,
        format_args!(
            "{{\"dhcp\":{},\"ip\":\"{}.{}.{}.{}\",\"subnet\":\"{}.{}.{}.{}\",\
             \"gateway\":\"{}.{}.{}.{}\",\"dns\":\"{}.{}.{}.{}\"}}",
            net.dhcp,
            net.ip[0],
            net.ip[1],
            net.ip[2],
            net.ip[3],
            net.subnet[0],
            net.subnet[1],
            net.subnet[2],
            net.subnet[3],
            net.gateway[0],
            net.gateway[1],
            net.gateway[2],
            net.gateway[3],
            net.dns[0],
            net.dns[1],
            net.dns[2],
            net.dns[3],
        ),
    );
    drop(guard);
    send_json_str(socket, body.as_str()).await;
}

async fn api_get_bacnet_config(socket: &mut TcpSocket<'_>) {
    let guard = CONFIG.lock().await;
    let cfg = match guard.as_ref() {
        Some(c) => c,
        None => {
            send_404(socket).await;
            return;
        }
    };
    let bac = &cfg.bacnet;
    let mut body: heapless::String<256> = heapless::String::new();
    let _ = core::fmt::write(
        &mut body,
        format_args!(
            "{{\"deviceId\":{},\"deviceName\":\"{}\",\"mstpMac\":{},\"mstpBaud\":{},\"maxMaster\":{}}}",
            bac.device_id,
            bac.device_name.as_str(),
            bac.mstp_mac,
            bac.mstp_baud,
            bac.max_master,
        ),
    );
    drop(guard);
    send_json_str(socket, body.as_str()).await;
}

async fn api_get_status(socket: &mut TcpSocket<'_>) {
    // Uptime in seconds via embassy_time::Instant
    let uptime_ms = embassy_time::Instant::now().as_millis();
    let uptime_s = uptime_ms / 1000;

    let device_count = BRIDGE_STATE.lock().await.device_count;

    let mut body: heapless::String<256> = heapless::String::new();
    let _ = core::fmt::write(
        &mut body,
        format_args!(
            "{{\"uptime\":{},\"deviceCount\":{},\"vendor\":\"Icomb Place\",\"firmware\":\"0.1.0\"}}",
            uptime_s, device_count,
        ),
    );
    send_json_str(socket, body.as_str()).await;
}

async fn api_put_network_config(socket: &mut TcpSocket<'_>, body: &str) {
    // Parse the JSON body into NetworkConfig using serde_json_core
    match serde_json_core::from_str::<bridge_core::config::NetworkConfig>(body) {
        Ok((net_cfg, _)) => {
            if let Some(cfg) = CONFIG.lock().await.as_mut() {
                cfg.network = net_cfg;
            }
            send_ok_empty(socket).await;
        }
        Err(_) => send_400(socket).await,
    }
}

async fn api_put_bacnet_config(socket: &mut TcpSocket<'_>, body: &str) {
    match serde_json_core::from_str::<bridge_core::config::BacnetDeviceConfig>(body) {
        Ok((bac_cfg, _)) => {
            if let Some(cfg) = CONFIG.lock().await.as_mut() {
                cfg.bacnet = bac_cfg;
            }
            send_ok_empty(socket).await;
        }
        Err(_) => send_400(socket).await,
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

async fn send_json(socket: &mut TcpSocket<'_>, body: &[u8]) {
    let mut hdr: heapless::String<128> = heapless::String::new();
    let _ = core::fmt::write(
        &mut hdr,
        format_args!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        ),
    );
    let _ = socket.write_all(hdr.as_bytes()).await;
    let _ = socket.write_all(body).await;
    let _ = socket.flush().await;
}

async fn send_json_str(socket: &mut TcpSocket<'_>, body: &str) {
    send_json(socket, body.as_bytes()).await;
}

async fn send_static_json(socket: &mut TcpSocket<'_>, body: &str) {
    send_json_str(socket, body).await;
}

async fn send_ok_empty(socket: &mut TcpSocket<'_>) {
    let _ = socket
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
        .await;
    let _ = socket.flush().await;
}

async fn send_400(socket: &mut TcpSocket<'_>) {
    let _ = socket
        .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
        .await;
    let _ = socket.flush().await;
}

async fn send_404(socket: &mut TcpSocket<'_>) {
    let _ = socket
        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
        .await;
    let _ = socket.flush().await;
}

// ---------------------------------------------------------------------------
// Minimal OpenAPI stub (served at /api/openapi.json)
// ---------------------------------------------------------------------------

const OPENAPI_STUB: &str = r#"{
  "openapi": "3.1.0",
  "info": {
    "title": "BACnet Bridge API",
    "version": "0.1.0",
    "contact": { "name": "Icomb Place" }
  },
  "paths": {}
}"#;
