#![no_std]

//! `bridge-core`: platform-independent BACnet MS/TP ↔ BACnet/IP bridge logic.
//!
//! This crate compiles for both the RP2350A target (`thumbv8m.main-none-eabihf`) and
//! the host (macOS/Linux) for unit testing. It has **no** hardware dependencies
//! and uses only `heapless`, `serde`, and `serde-json-core` — no `alloc`, no `std`.
//!
//! # Modules
//!
//! - [`bacnet`]    — BACnet PDU types (object IDs, property IDs, values, APDU/NPDU enums)
//! - [`bridge`]    — Bridge state: discovered devices and points (`BridgeStateInner`)
//! - [`npdu`]      — NPDU encode/decode
//! - [`config`]    — Configuration persistence types (`BridgeConfig`, `PointRule`)
//! - [`auth`]      — Password hashing, token verification, role-based permission checks
//! - [`pipeline`]  — Processor pipeline: value transformation and exposure routing
//! - [`mdns`]      — mDNS / DNS-SD packet codec
//! - [`ipc`]       — Inter-core ring buffer and `BacnetPdu` struct
//! - [`error`]     — Shared error types (`EncodeError`, `DecodeError`, `BridgeError`)
//! - [`ntp`]       — SNTP packet codec (RFC 4330)
//! - [`syslog`]    — RFC 5424 syslog message formatter
//! - [`ota`]       — OTA firmware update validation, UF2 parsing, and manifest parsing
//! - [`snmp`]      — Minimal SNMP v2c agent codec (RFC 3416)
//! - [`mqtt`]      — MQTT 3.1.1 publish-only client codec + HA auto-discovery
//! - [`tls`]       — TLS certificate management types (PEM→DER, CN extraction)

pub mod apdu;
pub mod auth;
pub mod bacnet;
pub mod bridge;
pub mod bvlc;
pub mod config;
pub mod error;
pub mod ipc;
pub mod mdns;
pub mod mqtt;
pub mod npdu;
pub mod ntp;
pub mod ota;
pub mod pipeline;
pub mod snmp;
pub mod syslog;
pub mod tls;

// Top-level re-exports of the most commonly used types.
pub use apdu::{
    decode_apdu, encode_error, encode_i_am, encode_read_property, encode_read_property_ack,
    encode_simple_ack, encode_subscribe_cov, encode_ucov_notification, encode_who_is,
    encode_write_property, CovNotification, DecodedApdu, IAmData, ReadPropertyAck,
    ReadPropertyRequest, SubscribeCovRequest, WhoIsRequest, WritePropertyRequest, SERVICE_I_AM,
    SERVICE_READ_PROPERTY, SERVICE_SUBSCRIBE_COV, SERVICE_UCOV_NOTIFICATION, SERVICE_WHO_IS,
    SERVICE_WRITE_PROPERTY,
};
pub use bacnet::{
    convert_for_exposure, convert_from_bacnet, convert_to_bacnet, convert_write_for_exposure,
    ApduType, BacnetValue, EngineeringUnits, Exposure, ObjectId, ObjectType, PointConfig,
    PropertyId, ServiceChoice,
};
pub use bridge::{BridgeStateInner, DeviceEntry, PointEntry, MAX_DEVICES, MAX_POINTS_PER_DEVICE};
pub use bvlc::{
    decode_bvlc, encode_bvlc, BvlcHeader, BACNET_IP_PORT, BVLC_HEADER_SIZE,
    BVLC_ORIGINAL_BROADCAST, BVLC_ORIGINAL_UNICAST, BVLC_TYPE,
};
pub use config::{
    BacnetDeviceConfig, BridgeConfig, Convertor, MqttConfig, NetworkConfig, NtpConfig, OtaConfig,
    PointMode, PointRule, Processor, SnmpConfig, SyslogConfig, TlsConfig, TokenConfig, UserConfig,
    UserRole,
};
pub use error::{BridgeError, DecodeError, EncodeError};
pub use ipc::{BacnetPdu, RingBuffer};
pub use mdns::{
    decode_query, encode_a_response, encode_ptr_response, encode_srv_response, encode_txt_response,
    DnsQuery, MDNS_ADDR, MDNS_PORT, TYPE_A, TYPE_PTR, TYPE_SRV, TYPE_TXT,
};
pub use mqtt::{
    decode_connack, decode_packet_type, encode_connect, encode_disconnect, encode_pingreq,
    encode_publish, format_ha_discovery, ha_discovery_topic, HaDiscoveryParams, MQTT_PORT,
    PACKET_TYPE_CONNACK, PACKET_TYPE_CONNECT, PACKET_TYPE_DISCONNECT, PACKET_TYPE_PINGREQ,
    PACKET_TYPE_PINGRESP, PACKET_TYPE_PUBLISH,
};
pub use npdu::{decode_npdu, encode_npdu, NpduHeader};
pub use ntp::{
    decode_packet as ntp_decode_packet, decode_response as ntp_decode_response,
    encode_request as ntp_encode_request, ntp_to_unix_epoch, NtpPacket, NtpTimestamp, NTP_PORT,
    NTP_UNIX_OFFSET, SNTP_PACKET_LEN,
};
pub use ota::{
    is_newer_version, is_uf2, parse_manifest, parse_uf2_block, validate_firmware_image,
    ManifestEntry, MAX_FIRMWARE_SIZE, UF2_BLOCK_SIZE, UF2_FAMILY_RP2040, UF2_MAGIC1, UF2_MAGIC2,
    UF2_MAGIC3, UF2_PAYLOAD_SIZE,
};
pub use snmp::{
    decode_get_request, encode_get_response, SnmpRequest, SnmpValue, VarBind, ERROR_GEN_ERR,
    ERROR_NO_ERROR, ERROR_NO_SUCH_NAME, OID_BACNET_DEVICES_DISCOVERED, OID_IPC_DROP_COUNT,
    OID_MSTP_FRAMES_RECV, OID_MSTP_FRAMES_SENT, OID_MSTP_TOKEN_LOSSES, OID_SYS_CONTACT,
    OID_SYS_DESCR, OID_SYS_NAME, OID_SYS_OBJECT_ID, OID_SYS_UPTIME, TAG_GET_NEXT_REQUEST,
    TAG_GET_REQUEST, TAG_GET_RESPONSE,
};
pub use syslog::{format_syslog, syslog_pri, SyslogFacility, SyslogSeverity};
pub use tls::{extract_subject_cn, is_cert_pem, is_key_pem, pem_to_der, MAX_CERT_PEM, MAX_KEY_DER};
