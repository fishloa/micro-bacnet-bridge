//! BACnet bridge routing task.
//!
//! This async task runs on Core 0 and is the central hub of the bridge:
//!
//! 1. Reads PDUs from the `mstp_to_ip` ring (frames received from the MS/TP bus)
//!    and processes them: I-Am → update device table, ReadProperty-ACK → update
//!    point values, COV-Notification → update point values.
//!
//! 2. Periodically sends Who-Is broadcasts via BACnet/IP to discover devices.
//!
//! 3. For discovered devices, issues ReadProperty requests to enumerate their
//!    points and read current values.
//!
//! 4. Handles incoming BACnet/IP requests (from the `ip_to_mstp` ring) that
//!    target our bridge's device object.

use crate::bridge::BRIDGE_STATE;
use crate::ipc;
use bridge_core::apdu::{
    self, decode_apdu, encode_i_am, encode_read_property, encode_who_is, DecodedApdu, IAmData,
    ReadPropertyRequest, SERVICE_READ_PROPERTY,
};
use bridge_core::bacnet::{BacnetValue, ObjectId, ObjectType, PropertyId};
use bridge_core::config::PointMode;
use bridge_core::ipc::BacnetPdu;
use bridge_core::npdu::decode_npdu;
use defmt::{info, warn};
use embassy_time::Timer;

/// Our bridge's BACnet device instance number (configurable, default 0xFFFF).
const BRIDGE_DEVICE_ID: u32 = 0x3FFFF;

/// BACnet vendor ID for Icomb Place (unregistered).
const VENDOR_ID: u16 = 0xFFFF;

/// Maximum APDU size we accept.
const MAX_APDU: u32 = 480;

/// Network number for the MS/TP side (0 = local).
#[allow(dead_code)]
const MSTP_NET: u16 = 1;

/// Network number for the BACnet/IP side (0 = local).
#[allow(dead_code)]
const BIP_NET: u16 = 2;

/// Monotonically increasing invoke ID for confirmed requests.
static INVOKE_ID: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(1);

fn next_invoke_id() -> u8 {
    INVOKE_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// Main bridge routing task.
#[embassy_executor::task]
pub async fn bacnet_router_task() {
    info!("bacnet_router: starting");

    // Initial Who-Is broadcast after a short delay
    Timer::after_secs(3).await;
    send_who_is_broadcast().await;

    let mut who_is_timer = 0u32;
    let mut read_timer = 0u32;

    loop {
        // Process PDUs from the MS/TP bus
        process_mstp_pdus().await;

        // Process PDUs from BACnet/IP (forwarded from ip_to_mstp for local handling)
        process_ip_pdus().await;

        // Periodic Who-Is every 30 seconds
        who_is_timer += 1;
        if who_is_timer >= 300 {
            who_is_timer = 0;
            send_who_is_broadcast().await;
        }

        // Periodic point polling every 5 seconds
        read_timer += 1;
        if read_timer >= 50 {
            read_timer = 0;
            poll_device_points().await;
        }

        Timer::after_millis(100).await;
    }
}

/// Process all pending PDUs from the MS/TP → IP ring.
async fn process_mstp_pdus() {
    let ring = ipc::mstp_to_ip();
    while let Some(pdu) = ring.pop() {
        if pdu.data_len == 0 {
            continue;
        }
        let data = &pdu.data[..pdu.data_len as usize];

        // Try to decode as NPDU + APDU
        let apdu_data = if let Ok((_npdu_hdr, apdu)) = decode_npdu(data) {
            apdu
        } else {
            // Raw APDU (no NPDU header) — some MS/TP frames are just APDU
            data
        };

        if apdu_data.is_empty() {
            continue;
        }

        match decode_apdu(apdu_data) {
            Ok(decoded) => handle_decoded_apdu(decoded, &pdu).await,
            Err(_) => {
                warn!("bacnet_router: failed to decode APDU from MS/TP");
            }
        }
    }
}

/// Process PDUs from BACnet/IP that target our bridge device.
async fn process_ip_pdus() {
    let ring = ipc::ip_to_mstp();
    while let Some(pdu) = ring.pop() {
        if pdu.data_len == 0 {
            continue;
        }
        let data = &pdu.data[..pdu.data_len as usize];

        if let Ok(decoded) = decode_apdu(data) {
            handle_decoded_apdu(decoded, &pdu).await;
        }
    }
}

/// Handle a decoded APDU from any source.
async fn handle_decoded_apdu(apdu: DecodedApdu, source_pdu: &BacnetPdu) {
    match apdu {
        DecodedApdu::IAm(iam) => {
            handle_i_am(&iam, source_pdu).await;
        }
        DecodedApdu::WhoIs(req) => {
            // If the request matches our device, respond with I-Am
            let our_id = BRIDGE_DEVICE_ID;
            let in_range = match (req.low_limit, req.high_limit) {
                (Some(lo), Some(hi)) => our_id >= lo && our_id <= hi,
                _ => true,
            };
            if in_range {
                send_i_am().await;
            }
        }
        DecodedApdu::ReadPropertyAck(ack, _invoke_id) => {
            handle_read_property_ack(&ack, source_pdu).await;
        }
        DecodedApdu::ReadProperty(req, invoke_id) => {
            // Someone is reading from our bridge device
            handle_read_property_request(&req, invoke_id, source_pdu).await;
        }
        DecodedApdu::UnconfirmedCovNotification(notif) => {
            handle_cov_notification(&notif).await;
        }
        DecodedApdu::WriteProperty(_req, _invoke_id) => {
            // TODO: handle writes to our device object
        }
        DecodedApdu::Error(invoke_id, service, class, code) => {
            warn!(
                "bacnet_router: error response invoke={} svc={} class={} code={}",
                invoke_id, service, class, code
            );
        }
        _ => {}
    }
}

/// Handle an I-Am response — register the device in bridge state.
async fn handle_i_am(iam: &IAmData, source_pdu: &BacnetPdu) {
    let device_id = iam.device_id.instance;

    // Ignore our own I-Am responses.
    if device_id == BRIDGE_DEVICE_ID {
        return;
    }

    let mac = source_pdu.source_mac[0];

    info!(
        "bacnet_router: I-Am from device {} (mac={}, vendor={})",
        device_id, mac, iam.vendor_id
    );

    let mut state = BRIDGE_STATE.lock().await;
    state.upsert_device(device_id, mac, "");
}

/// Handle a ReadProperty-ACK — update point value in bridge state.
///
/// Values pass through the configured processor pipeline (scale, offset,
/// state-text mapping, ignore/passthrough/processed modes) before being
/// stored in the bridge state.
async fn handle_read_property_ack(ack: &apdu::ReadPropertyAck, source_pdu: &BacnetPdu) {
    // Only update PresentValue for now
    if ack.property_id != PropertyId::PresentValue {
        return;
    }

    let unit = 0u16; // TODO: track engineering units from a prior ReadProperty

    // Find the responding device by its source MAC address.
    // Lock config first (read-only), then bridge state (write).
    let source_mac = source_pdu.source_mac[0];
    let cfg_guard = crate::http::CONFIG.lock().await;
    let mut state = BRIDGE_STATE.lock().await;
    for dev_idx in 0..state.device_count {
        if state.devices[dev_idx].mac == source_mac {
            let device_id = state.devices[dev_idx].device_id;
            let (mode, processors) = if let Some(cfg) = cfg_guard.as_ref() {
                cfg.find_point_rule_for_device(
                    device_id,
                    ack.object_id.object_type.code(),
                    ack.object_id.instance,
                )
            } else {
                (&PointMode::Passthrough, &[][..])
            };
            state.update_point_with_pipeline(
                dev_idx,
                ack.object_id,
                ack.value.clone(),
                unit,
                mode,
                processors,
            );
            break;
        }
    }
}

/// Handle a ReadProperty request targeting our bridge device.
async fn handle_read_property_request(
    req: &ReadPropertyRequest,
    invoke_id: u8,
    source_pdu: &BacnetPdu,
) {
    // Only respond for our own device object
    if req.object_id.object_type != ObjectType::Device || req.object_id.instance != BRIDGE_DEVICE_ID
    {
        return;
    }

    // These string literals are known-good ASCII and will always fit in String<64>.
    let value = match req.property_id {
        PropertyId::ObjectName => {
            let mut s: heapless::String<64> = heapless::String::new();
            let _ = s.push_str("BACnet Bridge");
            BacnetValue::CharString(s)
        }
        PropertyId::ObjectIdentifier => {
            BacnetValue::ObjectIdentifier(ObjectId::new(ObjectType::Device, BRIDGE_DEVICE_ID))
        }
        PropertyId::ObjectType => BacnetValue::Enumerated(8), // Device
        PropertyId::Description => {
            let mut s: heapless::String<64> = heapless::String::new();
            let _ = s.push_str("Icomb Place BACnet");
            BacnetValue::CharString(s)
        }
        _ => {
            // Send error: unknown property
            let mut buf = [0u8; 16];
            if let Ok(n) = apdu::encode_error(invoke_id, SERVICE_READ_PROPERTY, 2, 32, &mut buf) {
                send_reply(&buf[..n], source_pdu).await;
            }
            return;
        }
    };

    let ack = apdu::ReadPropertyAck {
        object_id: req.object_id,
        property_id: req.property_id,
        array_index: req.array_index,
        value,
    };

    let mut buf = [0u8; 128];
    if let Ok(n) = apdu::encode_read_property_ack(&ack, invoke_id, &mut buf) {
        send_reply(&buf[..n], source_pdu).await;
    }
}

/// Handle a COV notification — update point values through the pipeline.
async fn handle_cov_notification(notif: &apdu::CovNotification) {
    let device_id = notif.initiating_device.instance;

    let cfg_guard = crate::http::CONFIG.lock().await;
    let mut state = BRIDGE_STATE.lock().await;
    if let Some(dev_idx) = state.find_device(device_id) {
        let (mode, processors) = if let Some(cfg) = cfg_guard.as_ref() {
            cfg.find_point_rule_for_device(
                device_id,
                notif.monitored_object.object_type.code(),
                notif.monitored_object.instance,
            )
        } else {
            (&PointMode::Passthrough, &[][..])
        };
        for (prop, val) in &notif.values {
            if *prop == PropertyId::PresentValue {
                state.update_point_with_pipeline(
                    dev_idx,
                    notif.monitored_object,
                    val.clone(),
                    0,
                    mode,
                    processors,
                );
            }
        }
    }
}

/// Send a reply PDU back to the source.
async fn send_reply(apdu: &[u8], source_pdu: &BacnetPdu) {
    let mut pdu = BacnetPdu::new();

    // Swap source/dest from the original
    pdu.dest_mac = source_pdu.source_mac;
    pdu.dest_mac_len = source_pdu.source_mac_len;
    pdu.dest_net = source_pdu.source_net;

    let len = apdu.len().min(bridge_core::ipc::PDU_MAX_DATA);
    pdu.data[..len].copy_from_slice(&apdu[..len]);
    pdu.data_len = len as u16;
    pdu.pdu_type = if apdu.is_empty() { 0xFF } else { apdu[0] };

    // Send back via the same network the request came from.
    // MS/TP MACs are 1 byte; BACnet/IP MACs are 6 bytes (IP + port).
    if source_pdu.source_mac_len == 1 {
        // MS/TP source → reply via MS/TP (Core 0 → Core 1 outbound ring)
        let ring = ipc::ip_to_mstp();
        if !ring.push(&pdu) {
            warn!("bacnet_router: ip_to_mstp ring full");
        }
    } else {
        // BACnet/IP source → reply via BACnet/IP (outbound via UDP task)
        // The mstp_to_ip ring is consumed by bacnet_ip_task which sends UDP.
        let ring = ipc::mstp_to_ip();
        if !ring.push(&pdu) {
            warn!("bacnet_router: mstp_to_ip ring full");
        }
    }
}

/// Send an I-Am broadcast advertising our bridge device.
async fn send_i_am() {
    let iam = IAmData {
        device_id: ObjectId::new(ObjectType::Device, BRIDGE_DEVICE_ID),
        max_apdu: MAX_APDU,
        segmentation: 3, // no segmentation
        vendor_id: VENDOR_ID,
    };

    let mut buf = [0u8; 32];
    let n = match encode_i_am(&iam, &mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };

    let mut pdu = BacnetPdu::new();
    pdu.dest_mac_len = 0; // broadcast
    pdu.data[..n].copy_from_slice(&buf[..n]);
    pdu.data_len = n as u16;
    pdu.pdu_type = buf[0];

    // Broadcast via BACnet/IP
    let ring = ipc::mstp_to_ip();
    if !ring.push(&pdu) {
        warn!("bacnet_router: ring full, I-Am not sent");
    }
}

/// Send an unbounded Who-Is broadcast on BACnet/IP.
async fn send_who_is_broadcast() {
    let mut buf = [0u8; 8];
    let n = match encode_who_is(None, None, &mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };

    let mut pdu = BacnetPdu::new();
    pdu.dest_mac_len = 0; // broadcast
    pdu.data[..n].copy_from_slice(&buf[..n]);
    pdu.data_len = n as u16;
    pdu.pdu_type = buf[0];

    // Send via BACnet/IP
    let ring = ipc::mstp_to_ip();
    if ring.push(&pdu) {
        info!("bacnet_router: Who-Is broadcast sent");
    }
}

/// Poll discovered devices for their point values.
async fn poll_device_points() {
    let state = BRIDGE_STATE.lock().await;
    let count = state.device_count;
    if count == 0 {
        return;
    }

    // For each online device, send ReadProperty for PresentValue of known points
    for dev_idx in 0..count {
        let device = &state.devices[dev_idx];
        if !device.online {
            continue;
        }

        let points = state.get_device_points(dev_idx);
        for point in points.iter().flatten() {
            let req = ReadPropertyRequest {
                object_id: point.object_id,
                property_id: PropertyId::PresentValue,
                array_index: None,
            };

            let invoke_id = next_invoke_id();
            let mut buf = [0u8; 32];
            if let Ok(n) = encode_read_property(&req, invoke_id, &mut buf) {
                let mut pdu = BacnetPdu::new();
                pdu.dest_mac[0] = device.mac;
                pdu.dest_mac_len = 1;
                pdu.data[..n].copy_from_slice(&buf[..n]);
                pdu.data_len = n as u16;
                pdu.pdu_type = buf[0];

                let ring = ipc::ip_to_mstp();
                if !ring.push(&pdu) {
                    warn!("bacnet_router: outbound ring full");
                    return; // Stop polling if ring is full
                }
            }
        }
    }
}
