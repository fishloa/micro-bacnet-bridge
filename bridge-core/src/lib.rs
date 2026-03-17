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

pub mod bacnet;
pub mod config;
pub mod error;
pub mod ipc;
pub mod mdns;
pub mod npdu;

// Top-level re-exports of the most commonly used types.
pub use bacnet::{
    ApduType, BacnetValue, ObjectId, ObjectType, PropertyId, ServiceChoice,
};
pub use config::{BacnetDeviceConfig, BridgeConfig, NetworkConfig, UserConfig, UserRole};
pub use error::{BridgeError, DecodeError, EncodeError};
pub use ipc::{BacnetPdu, RingBuffer};
pub use mdns::{
    decode_query, encode_a_response, encode_ptr_response, encode_srv_response,
    encode_txt_response, DnsQuery, MDNS_ADDR, MDNS_PORT, TYPE_A, TYPE_PTR, TYPE_SRV,
    TYPE_TXT,
};
pub use npdu::{decode_npdu, encode_npdu, NpduHeader};
