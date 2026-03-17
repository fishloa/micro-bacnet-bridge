//! Configuration persistence types for the BACnet bridge.
//!
//! `BridgeConfig` is designed to be stored in the last flash sector using
//! the Pico SDK flash API. The `magic` field acts as a validity marker.

use heapless::{String, Vec};
use serde::{Deserialize, Serialize};

/// Magic number stored in every valid `BridgeConfig`.
/// Chosen as a memorable marker: 0xBAC0_CA1E ≈ "BACnet cable".
pub const MAGIC: u32 = 0xBAC0_CA1E;

/// Current schema version. Increment when fields are added/removed.
pub const CONFIG_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// NetworkConfig
// ---------------------------------------------------------------------------

/// Network / IP configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// If true, obtain IP via DHCP; otherwise use the static fields below.
    pub dhcp: bool,
    /// Static IPv4 address (used when `dhcp` is false, or as DHCP fallback).
    pub ip: [u8; 4],
    /// Subnet mask.
    pub subnet: [u8; 4],
    /// Default gateway.
    pub gateway: [u8; 4],
    /// DNS server.
    pub dns: [u8; 4],
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            dhcp: true,
            ip: [192, 168, 1, 100],
            subnet: [255, 255, 255, 0],
            gateway: [192, 168, 1, 1],
            dns: [8, 8, 8, 8],
        }
    }
}

// ---------------------------------------------------------------------------
// BacnetDeviceConfig
// ---------------------------------------------------------------------------

/// BACnet device and MS/TP configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacnetDeviceConfig {
    /// BACnet device instance number (0–4194302).
    pub device_id: u32,
    /// BACnet device name (Object_Name property of the Device object).
    pub device_name: String<32>,
    /// MS/TP MAC address (0–127).
    pub mstp_mac: u8,
    /// MS/TP baud rate: 9600, 19200, 38400, or 76800.
    pub mstp_baud: u32,
    /// Max Master value for the MS/TP token-passing loop (0–127).
    pub max_master: u8,
}

impl Default for BacnetDeviceConfig {
    fn default() -> Self {
        let mut device_name = String::new();
        // Infallible: "bacnet-bridge" is 13 chars, well within String<32>
        let _ = device_name.push_str("bacnet-bridge");
        Self {
            device_id: 389_999,
            device_name,
            mstp_mac: 1,
            mstp_baud: 76_800,
            max_master: 127,
        }
    }
}

// ---------------------------------------------------------------------------
// UserRole / UserConfig
// ---------------------------------------------------------------------------

/// User role controlling access to the admin UI and REST API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    /// Full read/write access including user management.
    Admin,
    /// Read-only access to device data and configuration.
    Viewer,
}

/// A single user account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserConfig {
    /// Login username (max 16 chars).
    pub username: String<16>,
    /// bcrypt password hash (24 bytes stores a 60-char bcrypt hash in base64 chunks;
    /// we store the raw 24-byte bcrypt output here for compactness).
    pub password_hash: [u8; 24],
    /// Access role.
    pub role: UserRole,
}

// ---------------------------------------------------------------------------
// BridgeConfig
// ---------------------------------------------------------------------------

/// Top-level bridge configuration struct, persisted to flash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Must equal `MAGIC` (0xBAC0_BRDG) for the config to be considered valid.
    pub magic: u32,
    /// Schema version. Must equal `CONFIG_VERSION`.
    pub version: u16,
    /// Network / IP settings.
    pub network: NetworkConfig,
    /// BACnet device and MS/TP settings.
    pub bacnet: BacnetDeviceConfig,
    /// mDNS hostname (advertised as `{hostname}.local`).
    pub hostname: String<32>,
    /// Configured user accounts (max 4).
    pub users: Vec<UserConfig, 4>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        let mut hostname = String::new();
        let _ = hostname.push_str("bacnet-bridge");
        Self {
            magic: MAGIC,
            version: CONFIG_VERSION,
            network: NetworkConfig::default(),
            bacnet: BacnetDeviceConfig::default(),
            hostname,
            users: Vec::new(),
        }
    }
}

impl BridgeConfig {
    /// Return true if the magic number and version are valid.
    pub fn validate(&self) -> bool {
        self.magic == MAGIC && self.version == CONFIG_VERSION
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_constant() {
        assert_eq!(MAGIC, 0xBAC0_CA1E);
    }

    #[test]
    fn default_validates() {
        let cfg = BridgeConfig::default();
        assert!(cfg.validate());
    }

    #[test]
    fn bad_magic_fails_validate() {
        let mut cfg = BridgeConfig::default();
        cfg.magic = 0xDEAD_BEEF;
        assert!(!cfg.validate());
    }

    #[test]
    fn bad_version_fails_validate() {
        let mut cfg = BridgeConfig::default();
        cfg.version = 99;
        assert!(!cfg.validate());
    }

    #[test]
    fn default_network_is_dhcp() {
        let cfg = BridgeConfig::default();
        assert!(cfg.network.dhcp);
    }

    #[test]
    fn default_bacnet_device_id() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.bacnet.device_id, 389_999);
        assert_eq!(cfg.bacnet.mstp_baud, 76_800);
        assert_eq!(cfg.bacnet.mstp_mac, 1);
        assert_eq!(cfg.bacnet.max_master, 127);
    }

    #[test]
    fn default_hostname() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.hostname.as_str(), "bacnet-bridge");
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let cfg = BridgeConfig::default();
        let mut buf = [0u8; 512];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(cfg, decoded);
        assert!(decoded.validate());
    }

    #[test]
    fn serialize_with_users() {
        let mut cfg = BridgeConfig::default();
        let mut username = String::new();
        let _ = username.push_str("admin");
        cfg.users
            .push(UserConfig {
                username,
                password_hash: [0xABu8; 24],
                role: UserRole::Admin,
            })
            .expect("push failed");

        let mut buf = [0u8; 1024];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(cfg, decoded);
        assert_eq!(decoded.users.len(), 1);
        assert_eq!(decoded.users[0].role, UserRole::Admin);
    }

    #[test]
    fn network_config_static_ip() {
        let mut cfg = BridgeConfig::default();
        cfg.network.dhcp = false;
        cfg.network.ip = [10, 0, 0, 50];
        cfg.network.subnet = [255, 255, 0, 0];

        let mut buf = [0u8; 512];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(!decoded.network.dhcp);
        assert_eq!(decoded.network.ip, [10, 0, 0, 50]);
        assert_eq!(decoded.network.subnet, [255, 255, 0, 0]);
    }
}
