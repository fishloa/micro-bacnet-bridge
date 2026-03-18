#![no_std]

//! `bridge-core`: platform-independent BACnet MS/TP ↔ BACnet/IP bridge logic.
//!
//! This crate compiles for both the RP2040 target (`thumbv6m-none-eabi`) and
//! the host (macOS/Linux) for unit testing. It has **no** hardware dependencies
//! and uses only `heapless`, `serde`, and `serde-json-core` — no `alloc`, no `std`.
//!
//! # Modules
//!
//! - [`bacnet`]  — BACnet PDU types (object IDs, property IDs, values, APDU/NPDU enums)
//! - [`npdu`]    — NPDU encode/decode
//! - [`config`]  — Configuration persistence types (`BridgeConfig`)
//! - [`mdns`]    — mDNS / DNS-SD packet codec
//! - [`ipc`]     — Inter-core ring buffer and `BacnetPdu` struct
//! - [`error`]   — Shared error types (`EncodeError`, `DecodeError`, `BridgeError`)
//! - [`ntp`]     — SNTP packet codec (RFC 4330)
//! - [`syslog`]  — RFC 5424 syslog message formatter
//! - [`snmp`]    — Minimal SNMP v2c agent codec (RFC 3416)
//! - [`mqtt`]    — MQTT 3.1.1 publish-only client codec + HA auto-discovery

pub mod bacnet;
pub mod config;
pub mod error;
pub mod ipc;
pub mod mdns;
pub mod mqtt;
pub mod npdu;
pub mod ntp;
pub mod snmp;
pub mod syslog;

// Top-level re-exports of the most commonly used types.
pub use bacnet::{
    ApduType, BacnetValue, EngineeringUnits, ObjectId, ObjectType, PropertyId, ServiceChoice,
};
pub use config::{
    BacnetDeviceConfig, BridgeConfig, NetworkConfig, PointConfig, UserConfig, UserRole,
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
    PACKET_TYPE_PUBLISH,
};
pub use npdu::{decode_npdu, encode_npdu, NpduHeader};
pub use ntp::{
    decode_packet as ntp_decode_packet, decode_response as ntp_decode_response,
    encode_request as ntp_encode_request, ntp_to_unix_epoch, NtpPacket, NtpTimestamp, NTP_PORT,
    NTP_UNIX_OFFSET, SNTP_PACKET_LEN,
};
pub use snmp::{
    decode_get_request, encode_get_response, SnmpRequest, SnmpValue, VarBind, ERROR_GEN_ERR,
    ERROR_NO_ERROR, ERROR_NO_SUCH_NAME, OID_BACNET_DEVICES_DISCOVERED, OID_IPC_DROP_COUNT,
    OID_MSTP_FRAMES_RECV, OID_MSTP_FRAMES_SENT, OID_MSTP_TOKEN_LOSSES, OID_SYS_CONTACT,
    OID_SYS_DESCR, OID_SYS_NAME, OID_SYS_UPTIME, TAG_GET_NEXT_REQUEST, TAG_GET_REQUEST,
    TAG_GET_RESPONSE,
};
pub use syslog::{format_syslog, syslog_pri, SyslogFacility, SyslogSeverity};
