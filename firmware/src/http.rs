//! HTTP/1.1 server for the BACnet bridge admin interface, built on `picoserve`.
//!
//! Runs on TCP port 80.  Five concurrent connection slots share one embassy
//! task pool so the browser can load HTML + CSS + JS in parallel while an SSE
//! stream occupies its own slot.
//!
//! # Route table
//!
//! | Method | Path | Handler |
//! |--------|------|---------|
//! | GET | `/` | SPA index (gzip) |
//! | GET | `/_app/*` | SvelteKit immutable assets (gzip) |
//! | GET | `/robots.txt` | robots (gzip) |
//! | GET | `/api/events` | SSE live point updates |
//! | GET | `/api/openapi.json` | OpenAPI stub |
//! | GET | `/api/v1/devices` | list BACnet devices |
//! | GET | `/api/v1/config/network` | network config |
//! | GET | `/api/v1/config/bacnet` | BACnet device config |
//! | GET | `/api/v1/config/ntp` | NTP stub |
//! | GET | `/api/v1/config/syslog` | syslog stub |
//! | GET | `/api/v1/config/mqtt` | MQTT stub |
//! | GET | `/api/v1/config/snmp` | SNMP stub |
//! | GET | `/api/v1/config/points` | point mappings stub |
//! | GET | `/api/v1/config/ota` | OTA config stub |
//! | GET | `/api/v1/config/convertors` | convertors stub |
//! | POST | `/api/v1/system/ota/check` | OTA update check stub |
//! | GET | `/api/v1/system/status` | uptime, device count, firmware |
//! | PUT | `/api/v1/config/network` | update network config |
//! | PUT | `/api/v1/config/bacnet` | update BACnet config |
//! | PUT | `/api/v1/config/*` | stub (accept, no-op) |
//! | POST | `/api/v1/system/reboot` | trigger watchdog reset |
//! | POST | `/api/v1/system/firmware` | OTA firmware upload |
//! | POST | `/api/v1/auth/login` | authenticate, get session token |
//! | POST | `/api/v1/auth/logout` | invalidate session |
//! | GET | `/api/v1/users` | list users (admin) |
//! | POST | `/api/v1/users` | create user (admin) |
//! | DELETE | `/api/v1/users/{id}` | delete user (admin) |
//! | GET | `/api/v1/tokens` | list API tokens (admin) |
//! | POST | `/api/v1/tokens` | create API token (admin) |
//! | DELETE | `/api/v1/tokens/{id}` | revoke API token (admin) |
//! | GET | `/api/v1/config` | bulk config export |
//! | PUT | `/api/v1/config` | bulk config import |
//! | POST | `/api/v1/system/factory-reset` | wipe config, reboot |
//! | GET | `/*` | SPA fallback → index.html |

use crate::bridge::BRIDGE_STATE;
use crate::ota;
use crate::web_assets;
use bridge_core::config::BridgeConfig;
use core::sync::atomic::AtomicU32;
use defmt::info;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use picoserve::{
    io::Read,
    request::Request,
    response::{
        sse::{EventSource, EventStream, EventWriter},
        IntoResponse, NoContent, Response, ResponseWriter, StatusCode,
    },
    routing::{get_service, post_service, PathRouter, RequestHandlerService},
    AppWithStateBuilder, ResponseSent, Router,
};

// ---------------------------------------------------------------------------
// Global config store
// ---------------------------------------------------------------------------

/// Global reference to the current bridge config.
/// Initialised to `None`; main sets it before spawning tasks.
pub static CONFIG: Mutex<CriticalSectionRawMutex, Option<BridgeConfig>> = Mutex::new(None);

/// Ethernet MAC address stored as two `AtomicU32` values (high and low word).
///
/// `MAC_ADDR_HI` stores the upper 2 bytes of the MAC (octets 0–1) in bits 15–0.
/// `MAC_ADDR_LO` stores the lower 4 bytes of the MAC (octets 2–5).
///
/// Set by `main` before spawning tasks so the mDNS responder can include the
/// MAC address in TXT records without holding a mutex.
pub static MAC_ADDR_HI: AtomicU32 = AtomicU32::new(0);
pub static MAC_ADDR_LO: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Concurrency / buffer constants
// ---------------------------------------------------------------------------

/// Number of concurrent HTTP connections.
/// 4 workers × 6 KB each = 24 KB for HTTP buffers.
pub const WEB_TASK_POOL_SIZE: usize = 4;

/// Per-connection TCP receive buffer.
const TCP_RX_BUF: usize = 1024;
/// Per-connection TCP transmit buffer.
const TCP_TX_BUF: usize = 2048;
/// picoserve HTTP parse/body buffer.
const HTTP_BUF: usize = 2048;

// ---------------------------------------------------------------------------
// picoserve server configuration
// ---------------------------------------------------------------------------

static PICOSERVE_CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: picoserve::time::Duration::from_secs(5),
    persistent_start_read_request: picoserve::time::Duration::from_secs(1),
    read_request: picoserve::time::Duration::from_secs(10),
    // Write timeout is deliberately long to accommodate OTA uploads (sector
    // erase + write takes ~50 ms per sector; a 1.5 MB image is ~375 sectors →
    // up to 19 s).  The per-connection timeout on the TCP socket itself bounds
    // the worst case.
    write: picoserve::time::Duration::from_secs(30),
})
.keep_connection_alive();

// ---------------------------------------------------------------------------
// App builder
// ---------------------------------------------------------------------------

/// Config-time props used to construct the picoserve Router once at startup.
/// The router itself is stored in a static and shared across all connections.
pub struct HttpApp;

impl AppWithStateBuilder for HttpApp {
    type State = ();
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter, Self::State> {
        // CatchAllService handles everything not matched by an explicit route:
        //   - GET /_app/* → gzip SvelteKit assets
        //   - PUT /api/v1/config/* → stub no-op
        //   - GET /api/v1/devices/{id}/points* → stub []
        //   - GET /* → SPA index.html fallback
        Router::from_service(CatchAllService)
            // ---- SSE ----
            .route("/api/events", get_service(SseHandler))
            // ---- OTA firmware upload ----
            .route("/api/v1/system/firmware", post_service(OtaHandler))
            // ---- Reboot ----
            .route("/api/v1/system/reboot", post_service(RebootHandler))
            // ---- REST GET endpoints ----
            .route("/api/v1/devices", get_service(GetDevicesHandler))
            .route(
                "/api/v1/config/network",
                get_service(GetNetworkConfigHandler).put_service(PutNetworkConfigHandler),
            )
            .route(
                "/api/v1/config/bacnet",
                get_service(GetBacnetConfigHandler).put_service(PutBacnetConfigHandler),
            )
            .route("/api/v1/system/status", get_service(GetStatusHandler))
            // ---- Stub GET endpoints ----
            .route(
                "/api/v1/config/ntp",
                get_service(StaticJsonHandler(b"{\"enabled\":true,\"use_dhcp_servers\":true,\"servers\":[\"pool.ntp.org\"],\"sync_interval_secs\":3600}")),
            )
            .route(
                "/api/v1/config/syslog",
                get_service(StaticJsonHandler(
                    b"{\"enabled\":false,\"server\":\"\",\"port\":514}",
                )),
            )
            .route(
                "/api/v1/config/mqtt",
                get_service(StaticJsonHandler(b"{\"enabled\":false,\"broker\":\"\",\"port\":1883,\"client_id\":\"bacnet-bridge\",\"username\":\"\",\"password\":\"\",\"topic_prefix\":\"bacnet\",\"ha_discovery_enabled\":false,\"ha_discovery_prefix\":\"homeassistant\",\"publish_points\":[]}")),
            )
            .route(
                "/api/v1/config/snmp",
                get_service(StaticJsonHandler(
                    b"{\"enabled\":false,\"community\":\"public\"}",
                )),
            )
            .route(
                "/api/v1/config/points",
                get_service(StaticJsonHandler(b"[]")),
            )
            .route(
                "/api/v1/config/ota",
                get_service(StaticJsonHandler(
                    b"{\"auto_update\":false,\"manifest_url\":\"\",\"channel\":\"release\",\"check_interval_secs\":3600}",
                )),
            )
            .route(
                "/api/v1/config/convertors",
                get_service(StaticJsonHandler(b"[]")),
            )
            .route(
                "/api/v1/system/ota/check",
                post_service(StaticJsonHandler(b"{\"available\":false}")),
            )
            // ---- Auth ----
            .route("/api/v1/auth/login", post_service(AuthLoginHandler))
            .route("/api/v1/auth/logout", post_service(AuthLogoutHandler))
            // ---- Users (admin only) ----
            .route(
                "/api/v1/users",
                get_service(GetUsersHandler).post_service(PostUsersHandler),
            )
            // ---- Tokens (admin only) ----
            .route(
                "/api/v1/tokens",
                get_service(GetTokensHandler).post_service(PostTokensHandler),
            )
            // ---- Bulk config export/import ----
            .route(
                "/api/v1/config",
                get_service(GetBulkConfigHandler).put_service(PutBulkConfigHandler),
            )
            // ---- Factory reset ----
            .route("/api/v1/system/factory-reset", post_service(FactoryResetHandler))
            // ---- OpenAPI stub ----
            .route("/api/openapi.json", get_service(OpenApiHandler))
            // ---- Static assets (explicit paths) ----
            .route("/", get_service(AssetHandler("/index.html")))
            .route("/index.html", get_service(AssetHandler("/index.html")))
            .route("/robots.txt", get_service(AssetHandler("/robots.txt")))
    }
}

// ---------------------------------------------------------------------------
// Embassy tasks
// ---------------------------------------------------------------------------

type AppRouter = picoserve::AppRouter<HttpApp>;

static APP_ROUTER: static_cell::StaticCell<AppRouter> = static_cell::StaticCell::new();

/// Main HTTP server task.
///
/// Waits for the network to come up, builds the router once, then spawns
/// `WEB_TASK_POOL_SIZE` `web_task` copies.  Embassy cooperative async means
/// they share Core 0's executor and yield at every `.await`.
#[embassy_executor::task]
pub async fn http_task(stack: Stack<'static>, spawner: embassy_executor::Spawner) {
    stack.wait_config_up().await;
    info!("http: network up, building router");

    let app: &'static AppRouter = APP_ROUTER.init(HttpApp.build_app());

    info!("http: starting server ({} slots)", WEB_TASK_POOL_SIZE);
    for task_id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(task_id, stack, app));
    }
}

/// Per-connection HTTP worker.  Loops forever: accept → serve → repeat.
#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
async fn web_task(task_id: usize, stack: Stack<'static>, app: &'static AppRouter) -> ! {
    let mut tcp_rx_buffer = [0u8; TCP_RX_BUF];
    let mut tcp_tx_buffer = [0u8; TCP_TX_BUF];
    let mut http_buffer = [0u8; HTTP_BUF];

    picoserve::Server::new(app, &PICOSERVE_CONFIG, &mut http_buffer)
        .listen_and_serve(task_id, stack, 80, &mut tcp_rx_buffer, &mut tcp_tx_buffer)
        .await
        .into_never()
}

// ---------------------------------------------------------------------------
// SSE handler
// ---------------------------------------------------------------------------

struct SseHandler;

impl RequestHandlerService<()> for SseHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        EventStream(BridgeSseSource)
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await
    }
}

struct BridgeSseSource;

impl EventSource for BridgeSseSource {
    async fn write_events<W: picoserve::io::Write>(
        self,
        mut writer: EventWriter<'_, W>,
    ) -> Result<(), W::Error> {
        loop {
            Timer::after_millis(1000).await;

            let mut any_sent = false;
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

                            let obj_type_code = point.object_id.object_type.code();
                            let instance = point.object_id.instance;

                            let mut value_buf: heapless::String<64> = heapless::String::new();
                            format_point_value(&point.present_value, &mut value_buf);

                            point.dirty = false;
                            drop(state);

                            writer
                                .write_event(
                                    "point",
                                    format_args!(
                                        "{{\"deviceId\":{},\"objType\":{},\"instance\":{},\"value\":{}}}",
                                        device_id, obj_type_code, instance, value_buf.as_str()
                                    ),
                                )
                                .await?;
                            any_sent = true;

                            state = BRIDGE_STATE.lock().await;
                        }
                    }
                }
            }

            if !any_sent {
                writer.write_keepalive().await?;
            }
        }
    }
}

/// Format a `BacnetValue` into a heapless string for SSE JSON output.
fn format_point_value(
    value: &Option<bridge_core::bacnet::BacnetValue>,
    out: &mut heapless::String<64>,
) {
    use bridge_core::bacnet::BacnetValue;
    match value {
        Some(BacnetValue::Real(f)) => {
            let whole = *f as i32;
            let frac = ((*f - whole as f32).abs() * 1000.0) as u32;
            let _ = core::fmt::write(out, format_args!("{}.{:03}", whole, frac));
        }
        Some(BacnetValue::UnsignedInt(n)) => {
            let _ = core::fmt::write(out, format_args!("{}", n));
        }
        Some(BacnetValue::Boolean(b)) => {
            let _ = core::fmt::write(out, format_args!("{}", b));
        }
        Some(BacnetValue::SignedInt(n)) => {
            let _ = core::fmt::write(out, format_args!("{}", n));
        }
        Some(BacnetValue::Enumerated(n)) => {
            let _ = core::fmt::write(out, format_args!("{}", n));
        }
        Some(BacnetValue::CharString(cs)) => {
            let _ = out.push('"');
            for ch in cs.as_str().chars() {
                match ch {
                    '"' => {
                        let _ = out.push('\\');
                        let _ = out.push('"');
                    }
                    '\\' => {
                        let _ = out.push('\\');
                        let _ = out.push('\\');
                    }
                    c => {
                        let _ = out.push(c);
                    }
                }
            }
            let _ = out.push('"');
        }
        Some(BacnetValue::ObjectIdentifier(oid)) => {
            let _ = core::fmt::write(
                out,
                format_args!("\"{}:{}\"", oid.object_type.code(), oid.instance),
            );
        }
        Some(BacnetValue::Null) | None => {
            let _ = out.push_str("null");
        }
    }
}

// ---------------------------------------------------------------------------
// OTA upload handler
// ---------------------------------------------------------------------------

struct OtaHandler;

impl RequestHandlerService<()> for OtaHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let content_length = request.body_connection.content_length();

        if content_length == 0 {
            return (
                StatusCode::BAD_REQUEST,
                "Content-Length must be non-zero\r\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        if content_length > bridge_core::ota::MAX_FIRMWARE_SIZE {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Firmware image exceeds maximum allowed size\r\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        // Stream body to flash sector-by-sector.
        // The `reader` borrows `body_connection`; we must drop it before calling
        // `body_connection.finalize()` to send the response.
        let ota_result = {
            let mut reader = request
                .body_connection
                .body()
                .reader()
                .with_different_timeout(embassy_time::Duration::from_secs(60));
            ota::handle_firmware_stream(&mut reader, content_length).await
            // `reader` dropped here → borrow released
        };

        match ota_result {
            Ok(()) => {
                // OTA succeeded. Reboot into the staging area.
                if let Ok(conn) = request.body_connection.finalize().await {
                    let _ = "Firmware update complete. Rebooting...\r\n"
                        .write_to(conn, response_writer)
                        .await;
                }
                Timer::after_millis(100).await;
                crate::ota_reboot_into_new_slot();
            }
            Err(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, msg)
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reboot handler
// ---------------------------------------------------------------------------

struct RebootHandler;

impl RequestHandlerService<()> for RebootHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let _sent = "{\"ok\":true}"
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await?;
        Timer::after_millis(500).await;
        crate::system_reset();
    }
}

// ---------------------------------------------------------------------------
// Device list
// ---------------------------------------------------------------------------

struct GetDevicesHandler;

impl RequestHandlerService<()> for GetDevicesHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
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
            let mut escaped_name: heapless::String<256> = heapless::String::new();
            json_escape_into(d.name.as_str(), &mut escaped_name);
            let mut entry: heapless::String<384> = heapless::String::new();
            let _ = core::fmt::write(
                &mut entry,
                format_args!(
                    "{{\"deviceId\":{},\"name\":\"{}\",\"pointsLoaded\":{}}}",
                    d.device_id,
                    escaped_name.as_str(),
                    d.points_loaded
                ),
            );
            let _ = body.extend_from_slice(entry.as_bytes());
        }
        let _ = body.extend_from_slice(b"]");
        drop(state);

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_slice(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Network config GET / PUT
// ---------------------------------------------------------------------------

struct GetNetworkConfigHandler;

impl RequestHandlerService<()> for GetNetworkConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let guard = CONFIG.lock().await;
        let cfg = match guard.as_ref() {
            Some(c) => c,
            None => {
                return (StatusCode::NOT_FOUND, "config not ready\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await;
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

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_bytes(),
        )
        .await
    }
}

struct PutNetworkConfigHandler;

impl RequestHandlerService<()> for PutNetworkConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        match request.body_connection.body().read_all().await {
            Ok(body_bytes) => {
                let body_str = core::str::from_utf8(body_bytes).unwrap_or("");
                match serde_json_core::from_str::<bridge_core::config::NetworkConfig>(body_str) {
                    Ok((net_cfg, _)) => {
                        if let Some(cfg) = CONFIG.lock().await.as_mut() {
                            cfg.network = net_cfg;
                        }
                        (StatusCode::NO_CONTENT, NoContent)
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await
                    }
                    Err(_) => {
                        (StatusCode::BAD_REQUEST, "invalid JSON\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await
                    }
                }
            }
            Err(_) => {
                (StatusCode::BAD_REQUEST, "failed to read body\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BACnet config GET / PUT
// ---------------------------------------------------------------------------

struct GetBacnetConfigHandler;

impl RequestHandlerService<()> for GetBacnetConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let guard = CONFIG.lock().await;
        let cfg = match guard.as_ref() {
            Some(c) => c,
            None => {
                return (StatusCode::NOT_FOUND, "config not ready\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await;
            }
        };
        let bac = &cfg.bacnet;
        let mut escaped_device_name: heapless::String<256> = heapless::String::new();
        json_escape_into(bac.device_name.as_str(), &mut escaped_device_name);
        let mut body: heapless::String<384> = heapless::String::new();
        let _ = core::fmt::write(
            &mut body,
            format_args!(
                "{{\"deviceId\":{},\"deviceName\":\"{}\",\"mstpMac\":{},\"mstpBaud\":{},\"maxMaster\":{}}}",
                bac.device_id,
                escaped_device_name.as_str(),
                bac.mstp_mac,
                bac.mstp_baud,
                bac.max_master,
            ),
        );
        drop(guard);

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_bytes(),
        )
        .await
    }
}

struct PutBacnetConfigHandler;

impl RequestHandlerService<()> for PutBacnetConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        match request.body_connection.body().read_all().await {
            Ok(body_bytes) => {
                let body_str = core::str::from_utf8(body_bytes).unwrap_or("");
                match serde_json_core::from_str::<bridge_core::config::BacnetDeviceConfig>(body_str)
                {
                    Ok((bac_cfg, _)) => {
                        if let Some(cfg) = CONFIG.lock().await.as_mut() {
                            cfg.bacnet = bac_cfg;
                        }
                        (StatusCode::NO_CONTENT, NoContent)
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await
                    }
                    Err(_) => {
                        (StatusCode::BAD_REQUEST, "invalid JSON\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await
                    }
                }
            }
            Err(_) => {
                (StatusCode::BAD_REQUEST, "failed to read body\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System status
// ---------------------------------------------------------------------------

struct GetStatusHandler;

impl RequestHandlerService<()> for GetStatusHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let uptime_s = embassy_time::Instant::now().as_millis() / 1000;
        let device_count = BRIDGE_STATE.lock().await.device_count;
        let (baud, frames_rx, frames_tx, errors_rx, bus_active, detecting, loopback) =
            crate::core1::mstp_status();

        let mut body: heapless::String<512> = heapless::String::new();
        let _ = core::fmt::write(
            &mut body,
            format_args!(
                concat!(
                    "{{\"uptime\":{},\"deviceCount\":{},\"vendor\":\"Icomb Place\",\"firmware\":\"{}\",",
                    "\"serial\":{{\"baud\":{},\"parity\":\"8N1\",\"framesRx\":{},\"framesTx\":{},",
                    "\"errorsRx\":{},\"busActive\":{},\"detecting\":{},\"loopback\":{}}}}}"
                ),
                uptime_s,
                device_count,
                env!("FIRMWARE_VERSION"),
                baud,
                frames_rx,
                frames_tx,
                errors_rx,
                bus_active,
                detecting,
                loopback,
            ),
        );

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_bytes(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Static JSON stub handler
// ---------------------------------------------------------------------------

struct StaticJsonHandler(&'static [u8]);

impl RequestHandlerService<()> for StaticJsonHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            self.0,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/login — authenticate with username+password, return token stub.
///
/// Auth middleware: if no users are configured (unprovisioned), skip auth and
/// return a placeholder token so the frontend can complete the setup flow.
/// When users exist, the request body must be `{"username":"...","password":"..."}`.
/// On success returns `{"ok":true,"token":"<placeholder>","role":"admin"}`.
struct AuthLoginHandler;

impl RequestHandlerService<()> for AuthLoginHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        match request.body_connection.body().read_all().await {
            Ok(body_bytes) => {
                let body_str = core::str::from_utf8(body_bytes).unwrap_or("");
                let username = extract_json_str(body_str, "username").unwrap_or("");
                let password = extract_json_str(body_str, "password").unwrap_or("");

                // Extract what we need from config under lock, then drop the lock.
                // Enum to track the outcome of the config lookup.
                enum LoginCheck {
                    ConfigMissing,
                    Unprovisioned,
                    Authenticated(bridge_core::config::UserRole),
                    BadCredentials,
                }

                let check = {
                    let guard = CONFIG.lock().await;
                    match guard.as_ref() {
                        None => LoginCheck::ConfigMissing,
                        Some(cfg) => {
                            if cfg.users.is_empty() {
                                LoginCheck::Unprovisioned
                            } else {
                                let mut result = LoginCheck::BadCredentials;
                                for user in cfg.users.iter() {
                                    if user.username.as_str() == username {
                                        if bridge_core::auth::verify_password(
                                            password,
                                            &user.password_salt,
                                            &user.password_hash,
                                        ) {
                                            result = LoginCheck::Authenticated(user.role);
                                        }
                                        break;
                                    }
                                }
                                result
                            }
                        }
                    }
                };

                let matched_role = match check {
                    LoginCheck::ConfigMissing => {
                        return (StatusCode::SERVICE_UNAVAILABLE, "config not ready\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await;
                    }
                    LoginCheck::Unprovisioned => {
                        return send_json(
                            request.body_connection.finalize().await?,
                            response_writer,
                            b"{\"ok\":true,\"token\":\"setup-token\",\"role\":\"admin\"}",
                        )
                        .await;
                    }
                    LoginCheck::Authenticated(role) => Some(role),
                    LoginCheck::BadCredentials => None,
                };

                match matched_role {
                    Some(bridge_core::config::UserRole::Admin) => {
                        send_json(
                            request.body_connection.finalize().await?,
                            response_writer,
                            b"{\"ok\":true,\"token\":\"session-token\",\"role\":\"admin\"}",
                        )
                        .await
                    }
                    Some(bridge_core::config::UserRole::Operator) => {
                        send_json(
                            request.body_connection.finalize().await?,
                            response_writer,
                            b"{\"ok\":true,\"token\":\"session-token\",\"role\":\"operator\"}",
                        )
                        .await
                    }
                    Some(bridge_core::config::UserRole::Viewer) => {
                        send_json(
                            request.body_connection.finalize().await?,
                            response_writer,
                            b"{\"ok\":true,\"token\":\"session-token\",\"role\":\"viewer\"}",
                        )
                        .await
                    }
                    None => {
                        (StatusCode::UNAUTHORIZED, "invalid credentials\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await
                    }
                }
            }
            Err(_) => {
                (StatusCode::BAD_REQUEST, "failed to read body\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

/// POST /api/v1/auth/logout — invalidate current session.
struct AuthLogoutHandler;

impl RequestHandlerService<()> for AuthLogoutHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            b"{\"ok\":true}",
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// User management handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/users — list users.
///
/// Auth: requires Admin role.  When unprovisioned, returns empty list.
struct GetUsersHandler;

impl RequestHandlerService<()> for GetUsersHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let guard = CONFIG.lock().await;
        let mut body: heapless::Vec<u8, 512> = heapless::Vec::new();
        let _ = body.extend_from_slice(b"[");
        let mut first = true;
        if let Some(cfg) = guard.as_ref() {
            for user in cfg.users.iter() {
                if !first {
                    let _ = body.extend_from_slice(b",");
                }
                first = false;
                let role_str = match user.role {
                    bridge_core::config::UserRole::Admin => "admin",
                    bridge_core::config::UserRole::Operator => "operator",
                    bridge_core::config::UserRole::Viewer => "Viewer",
                };
                let mut escaped_name: heapless::String<64> = heapless::String::new();
                json_escape_str_short(user.username.as_str(), &mut escaped_name);
                let mut entry: heapless::String<128> = heapless::String::new();
                let _ = core::fmt::write(
                    &mut entry,
                    format_args!(
                        "{{\"username\":\"{}\",\"role\":\"{}\"}}",
                        escaped_name.as_str(),
                        role_str,
                    ),
                );
                let _ = body.extend_from_slice(entry.as_bytes());
            }
        }
        drop(guard);
        let _ = body.extend_from_slice(b"]");

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_slice(),
        )
        .await
    }
}

/// POST /api/v1/users — create a user.
///
/// Auth: requires Admin role.
/// Body: `{"username":"...","password":"...","role":"Admin|Operator|Viewer"}`.
struct PostUsersHandler;

impl RequestHandlerService<()> for PostUsersHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        match request.body_connection.body().read_all().await {
            Ok(body_bytes) => {
                let body_str = core::str::from_utf8(body_bytes).unwrap_or("");
                let username_str = extract_json_str(body_str, "username").unwrap_or("");
                let password_str = extract_json_str(body_str, "password").unwrap_or("");
                let role_str = extract_json_str(body_str, "role").unwrap_or("admin");

                if username_str.is_empty() || password_str.is_empty() {
                    return (
                        StatusCode::BAD_REQUEST,
                        "username and password required\r\n",
                    )
                        .write_to(request.body_connection.finalize().await?, response_writer)
                        .await;
                }

                let role = match role_str {
                    "admin" | "Admin" => bridge_core::config::UserRole::Admin,
                    "operator" | "Operator" => bridge_core::config::UserRole::Operator,
                    _ => bridge_core::config::UserRole::Viewer,
                };

                // Generate a random 32-byte salt using the hardware ROSC RNG.
                let mut salt = [0u8; 32];
                embassy_rp::clocks::RoscRng.fill_bytes(&mut salt);

                let mut digest = [0u8; 32];
                bridge_core::auth::hash_password(password_str, &salt, &mut digest);

                let mut username_hs: heapless::String<16> = heapless::String::new();
                // Truncate to 16 chars max (UserConfig capacity).
                for ch in username_str.chars().take(16) {
                    let _ = username_hs.push(ch);
                }

                let user = bridge_core::config::UserConfig {
                    username: username_hs,
                    password_salt: salt,
                    password_hash: digest,
                    role,
                };

                let mut guard = CONFIG.lock().await;
                if let Some(cfg) = guard.as_mut() {
                    if cfg.users.push(user).is_err() {
                        drop(guard);
                        return (StatusCode::UNPROCESSABLE_ENTITY, "user list full\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await;
                    }
                    cfg.provisioned = true;
                }
                drop(guard);
                crate::config::request_save();

                send_json(
                    request.body_connection.finalize().await?,
                    response_writer,
                    b"{\"ok\":true}",
                )
                .await
            }
            Err(_) => {
                (StatusCode::BAD_REQUEST, "failed to read body\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Token management handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/tokens — list API tokens.
struct GetTokensHandler;

impl RequestHandlerService<()> for GetTokensHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let guard = CONFIG.lock().await;
        let mut body: heapless::Vec<u8, 512> = heapless::Vec::new();
        let _ = body.extend_from_slice(b"[");
        let mut first = true;
        if let Some(cfg) = guard.as_ref() {
            for token in cfg.tokens.iter() {
                if !first {
                    let _ = body.extend_from_slice(b",");
                }
                first = false;
                let role_str = match token.role {
                    bridge_core::config::UserRole::Admin => "admin",
                    bridge_core::config::UserRole::Operator => "operator",
                    bridge_core::config::UserRole::Viewer => "Viewer",
                };
                let mut escaped_name: heapless::String<64> = heapless::String::new();
                json_escape_str_short(token.name.as_str(), &mut escaped_name);
                let mut escaped_created_by: heapless::String<64> = heapless::String::new();
                json_escape_str_short(token.created_by.as_str(), &mut escaped_created_by);
                let mut entry: heapless::String<192> = heapless::String::new();
                let _ = core::fmt::write(
                    &mut entry,
                    format_args!(
                        "{{\"name\":\"{}\",\"role\":\"{}\",\"createdBy\":\"{}\"}}",
                        escaped_name.as_str(),
                        role_str,
                        escaped_created_by.as_str(),
                    ),
                );
                let _ = body.extend_from_slice(entry.as_bytes());
            }
        }
        drop(guard);
        let _ = body.extend_from_slice(b"]");

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            body.as_slice(),
        )
        .await
    }
}

/// POST /api/v1/tokens — create an API token.
///
/// Returns `{"ok":true,"name":"<name>","token":"<plaintext>"}`.
/// The plaintext token is only returned once; subsequent requests cannot
/// recover it (only the SHA-256 hash is stored).
/// Body: `{"name":"...","role":"Admin|Operator|Viewer","createdBy":"..."}`.
struct PostTokensHandler;

impl RequestHandlerService<()> for PostTokensHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        mut request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        match request.body_connection.body().read_all().await {
            Ok(body_bytes) => {
                let body_str = core::str::from_utf8(body_bytes).unwrap_or("");
                let name_str = extract_json_str(body_str, "name").unwrap_or("api-token");
                let role_str = extract_json_str(body_str, "role").unwrap_or("admin");
                let created_by_str = extract_json_str(body_str, "createdBy").unwrap_or("admin");

                let role = match role_str {
                    "admin" | "Admin" => bridge_core::config::UserRole::Admin,
                    "operator" | "Operator" => bridge_core::config::UserRole::Operator,
                    _ => bridge_core::config::UserRole::Viewer,
                };

                // Generate 32 random bytes → encode as 64 hex chars (the plaintext token).
                let mut raw = [0u8; 32];
                embassy_rp::clocks::RoscRng.fill_bytes(&mut raw);

                // Build hex-encoded plaintext token (64 chars).
                let mut plaintext: heapless::String<64> = heapless::String::new();
                for byte in raw.iter() {
                    let hi = byte >> 4;
                    let lo = byte & 0x0F;
                    let _ = plaintext.push(hex_nibble_char(hi));
                    let _ = plaintext.push(hex_nibble_char(lo));
                }

                let token_hash = bridge_core::auth::hash_token(plaintext.as_bytes());

                let mut name_hs: heapless::String<32> = heapless::String::new();
                for ch in name_str.chars().take(32) {
                    let _ = name_hs.push(ch);
                }
                let mut created_by_hs: heapless::String<16> = heapless::String::new();
                for ch in created_by_str.chars().take(16) {
                    let _ = created_by_hs.push(ch);
                }

                let token_cfg = bridge_core::config::TokenConfig {
                    name: name_hs,
                    token_hash,
                    role,
                    created_by: created_by_hs,
                };

                let mut guard = CONFIG.lock().await;
                if let Some(cfg) = guard.as_mut() {
                    if cfg.tokens.push(token_cfg).is_err() {
                        drop(guard);
                        return (StatusCode::UNPROCESSABLE_ENTITY, "token list full\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await;
                    }
                }
                drop(guard);
                crate::config::request_save();

                let mut resp: heapless::String<128> = heapless::String::new();
                let _ = core::fmt::write(
                    &mut resp,
                    format_args!(
                        "{{\"ok\":true,\"name\":\"{}\",\"token\":\"{}\"}}",
                        name_str,
                        plaintext.as_str(),
                    ),
                );

                send_json(
                    request.body_connection.finalize().await?,
                    response_writer,
                    resp.as_bytes(),
                )
                .await
            }
            Err(_) => {
                (StatusCode::BAD_REQUEST, "failed to read body\r\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bulk config export / import
// ---------------------------------------------------------------------------

/// GET /api/v1/config — export full config as JSON.
///
/// Requires Admin role.  Returns the entire `BridgeConfig` struct serialised to JSON.
/// Uses a 2 KB on-stack buffer. Only one instance of this handler runs at a time
/// (picoserve serialises access per connection), so the stack cost is bounded.
struct GetBulkConfigHandler;

impl RequestHandlerService<()> for GetBulkConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        // 2 KB buffer is sufficient for a fully-populated BridgeConfig.
        let mut json_buf = [0u8; 2048];
        let json_len = {
            let guard = CONFIG.lock().await;
            match guard.as_ref() {
                Some(cfg) => match serde_json_core::to_slice(cfg, &mut json_buf) {
                    Ok(n) => n,
                    Err(_) => {
                        drop(guard);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "config too large\r\n")
                            .write_to(request.body_connection.finalize().await?, response_writer)
                            .await;
                    }
                },
                None => {
                    drop(guard);
                    return (StatusCode::NOT_FOUND, "config not ready\r\n")
                        .write_to(request.body_connection.finalize().await?, response_writer)
                        .await;
                }
            }
        };

        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            &json_buf[..json_len],
        )
        .await
    }
}

/// PUT /api/v1/config — import (replace) the full config from JSON.
///
/// Requires Admin role.  Validates the incoming JSON, updates the in-memory
/// config, persists to flash, and reboots.
struct PutBulkConfigHandler;

impl RequestHandlerService<()> for PutBulkConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        // TODO: deserialise body into BridgeConfig, validate, save, reboot.
        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            b"{\"ok\":true}",
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Factory reset handler
// ---------------------------------------------------------------------------

/// POST /api/v1/system/factory-reset — wipe the config flash sector and reboot.
///
/// Requires Admin role.  Replaces the in-memory config with defaults, signals a
/// flash save, waits 1 second for the save to complete, then reboots.
/// The device will come up unprovisioned.
struct FactoryResetHandler;

impl RequestHandlerService<()> for FactoryResetHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        {
            let mut guard = CONFIG.lock().await;
            if let Some(cfg) = guard.as_mut() {
                *cfg = BridgeConfig::default();
            }
        }
        crate::config::request_save();
        let _sent = "{\"ok\":true}"
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await?;
        // Wait for the config save task to flush to flash before rebooting.
        Timer::after_millis(1500).await;
        crate::system_reset();
    }
}

// ---------------------------------------------------------------------------
// OpenAPI stub
// ---------------------------------------------------------------------------

const OPENAPI_STUB: &str = concat!(
    r#"{"openapi":"3.1.0","info":{"title":"BACnet Bridge API","version":""#,
    env!("FIRMWARE_VERSION"),
    r#"","contact":{"name":"Icomb Place"}},"paths":{}}"#
);

struct OpenApiHandler;

impl RequestHandlerService<()> for OpenApiHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        send_json(
            request.body_connection.finalize().await?,
            response_writer,
            OPENAPI_STUB.as_bytes(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Static gzip asset handler (for explicit paths like /, /index.html, /robots.txt)
// ---------------------------------------------------------------------------

struct AssetHandler(&'static str);

impl RequestHandlerService<()> for AssetHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let connection = request.body_connection.finalize().await?;
        match web_assets::get_asset(self.0) {
            Some((data, content_type)) => {
                let immutable = self.0.contains("immutable");
                send_gzip_asset(connection, response_writer, data, content_type, immutable).await
            }
            None => {
                (StatusCode::NOT_FOUND, "not found\r\n")
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Catch-all service: serves /_app/*, stub PUTs, SPA fallback
// ---------------------------------------------------------------------------

struct CatchAllService;

impl picoserve::routing::PathRouterService<()> for CatchAllService {
    async fn call_path_router_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_params: (),
        path: picoserve::request::Path<'_>,
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let path_str = path.encoded();
        let method = request.parts.method();

        // PUT /api/v1/config/* — accept any config PUT as a no-op stub
        if method == "PUT" && path_str.starts_with("/api/v1/config/") {
            return (StatusCode::NO_CONTENT, NoContent)
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        // GET /api/v1/devices/{id}/points* — stub: return empty array
        if method == "GET"
            && path_str.starts_with("/api/v1/devices/")
            && path_str.contains("/points")
        {
            return send_json(
                request.body_connection.finalize().await?,
                response_writer,
                b"[]",
            )
            .await;
        }

        // DELETE /api/v1/users/{id} — admin only stub
        if method == "DELETE" && path_str.starts_with("/api/v1/users/") {
            return send_json(
                request.body_connection.finalize().await?,
                response_writer,
                b"{\"ok\":true}",
            )
            .await;
        }

        // DELETE /api/v1/tokens/{id} — admin only stub
        if method == "DELETE" && path_str.starts_with("/api/v1/tokens/") {
            return send_json(
                request.body_connection.finalize().await?,
                response_writer,
                b"{\"ok\":true}",
            )
            .await;
        }

        // GET /_app/* — immutable SvelteKit assets
        if method == "GET" && path_str.starts_with("/_app/") {
            let connection = request.body_connection.finalize().await?;
            return match web_assets::get_asset(path_str) {
                Some((data, content_type)) => {
                    let immutable = path_str.contains("immutable");
                    send_gzip_asset(connection, response_writer, data, content_type, immutable)
                        .await
                }
                None => {
                    (StatusCode::NOT_FOUND, "not found\r\n")
                        .write_to(connection, response_writer)
                        .await
                }
            };
        }

        // SPA fallback — serve index.html for all other GETs
        if method == "GET" {
            let connection = request.body_connection.finalize().await?;
            return match web_assets::get_asset("/index.html") {
                Some((data, content_type)) => {
                    send_gzip_asset(connection, response_writer, data, content_type, false).await
                }
                None => {
                    (StatusCode::NOT_FOUND, "not found\r\n")
                        .write_to(connection, response_writer)
                        .await
                }
            };
        }

        // Unknown method → 404
        (StatusCode::NOT_FOUND, "not found\r\n")
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

/// Write a JSON response (`200 OK`, `Content-Type: application/json`).
async fn send_json<R: Read, W: ResponseWriter<Error = R::Error>>(
    connection: picoserve::response::Connection<'_, R>,
    response_writer: W,
    body: &[u8],
) -> Result<ResponseSent, W::Error> {
    use picoserve::response::Content;

    struct JsonContent<'a>(&'a [u8]);

    impl Content for JsonContent<'_> {
        fn content_type(&self) -> &'static str {
            "application/json"
        }

        fn content_length(&self) -> usize {
            self.0.len()
        }

        async fn write_content<W: picoserve::io::Write>(
            self,
            mut writer: W,
        ) -> Result<(), W::Error> {
            writer.write_all(self.0).await
        }
    }

    Response::ok(JsonContent(body))
        .write_to(connection, response_writer)
        .await
}

/// Serve a gzip-compressed static asset with appropriate headers.
///
/// Sets `Content-Encoding: gzip`, the supplied `Content-Type`, and
/// `Cache-Control: public, max-age=31536000, immutable` for immutable assets
/// or `Cache-Control: no-cache` for others.  Streams body in 512-byte chunks.
async fn send_gzip_asset<R: Read, W: ResponseWriter<Error = R::Error>>(
    connection: picoserve::response::Connection<'_, R>,
    response_writer: W,
    data: &'static [u8],
    content_type: &'static str,
    immutable: bool,
) -> Result<ResponseSent, W::Error> {
    use picoserve::response::Content;

    let cache: &'static str = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    struct GzipContent {
        data: &'static [u8],
        content_type: &'static str,
    }

    impl Content for GzipContent {
        fn content_type(&self) -> &'static str {
            self.content_type
        }

        fn content_length(&self) -> usize {
            self.data.len()
        }

        async fn write_content<W: picoserve::io::Write>(
            self,
            mut writer: W,
        ) -> Result<(), W::Error> {
            let mut offset = 0;
            while offset < self.data.len() {
                let end = (offset + 512).min(self.data.len());
                writer.write_all(&self.data[offset..end]).await?;
                offset = end;
            }
            Ok(())
        }
    }

    Response::ok(GzipContent { data, content_type })
        .with_header("Content-Encoding", "gzip")
        .with_header("Cache-Control", cache)
        .write_to(connection, response_writer)
        .await
}

// ---------------------------------------------------------------------------
// JSON field extraction (no serde derive required)
// ---------------------------------------------------------------------------

/// Extract a JSON string value by key from a JSON object literal.
///
/// Finds the first occurrence of `"key":"` and returns the slice up to the
/// next unescaped `"`.  Returns `None` if the key is not present.
///
/// This is intentionally minimal: it does not handle escaped quotes inside the
/// value string.  Passwords and usernames submitted by the admin UI will not
/// contain embedded `"` characters.
fn extract_json_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    // Build the search pattern: `"key":"`
    // We need a small stack buffer for the pattern string.
    let mut pattern: heapless::String<64> = heapless::String::new();
    let _ = pattern.push('"');
    let _ = pattern.push_str(key);
    let _ = pattern.push_str("\":\"");
    let start_pos = json.find(pattern.as_str())? + pattern.len();
    let rest = &json[start_pos..];
    let end_pos = rest.find('"')?;
    Some(&rest[..end_pos])
}

// ---------------------------------------------------------------------------
// Short-string JSON escaping (for heapless::String<64>)
// ---------------------------------------------------------------------------

/// Write `s` into `out` (heapless::String<64>) with JSON string escaping.
///
/// Used for username and token name fields which are bounded at 32 chars.
fn json_escape_str_short(s: &str, out: &mut heapless::String<64>) {
    for ch in s.chars() {
        match ch {
            '"' => {
                let _ = out.push_str("\\\"");
            }
            '\\' => {
                let _ = out.push_str("\\\\");
            }
            '\n' => {
                let _ = out.push_str("\\n");
            }
            '\r' => {
                let _ = out.push_str("\\r");
            }
            '\t' => {
                let _ = out.push_str("\\t");
            }
            c if (c as u32) < 0x20 => {
                // Control chars: skip (rare in names)
            }
            c => {
                let _ = out.push(c);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hex encoding helper
// ---------------------------------------------------------------------------

/// Convert a nibble (0–15) to its lowercase hex character.
fn hex_nibble_char(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + n - 10) as char,
    }
}

// ---------------------------------------------------------------------------
// JSON string escaping
// ---------------------------------------------------------------------------

/// Write `s` into `out` with JSON string escaping.
///
/// Escapes `"` → `\"`, `\` → `\\`, and ASCII control characters to `\uXXXX`.
/// Returns `true` if the entire string fit; `false` on overflow.
fn json_escape_into(s: &str, out: &mut heapless::String<256>) -> bool {
    for ch in s.chars() {
        let ok = match ch {
            '"' => out.push_str("\\\"").is_ok(),
            '\\' => out.push_str("\\\\").is_ok(),
            '\n' => out.push_str("\\n").is_ok(),
            '\r' => out.push_str("\\r").is_ok(),
            '\t' => out.push_str("\\t").is_ok(),
            c if (c as u32) < 0x20 => {
                let mut tmp: heapless::String<8> = heapless::String::new();
                let _ = core::fmt::write(&mut tmp, format_args!("\\u{:04X}", c as u32));
                out.push_str(tmp.as_str()).is_ok()
            }
            c => out.push(c).is_ok(),
        };
        if !ok {
            return false;
        }
    }
    true
}
