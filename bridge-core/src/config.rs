//! Configuration persistence types for the BACnet bridge.
//!
//! `BridgeConfig` is designed to be stored in the last flash sector using
//! the Pico SDK flash API. The `magic` field acts as a validity marker.
//!
//! Schema version 5 — replaces inline processors/ExposureConfig with Convertor table.

use heapless::{String, Vec};
use serde::{Deserialize, Serialize};

/// Magic number stored in every valid `BridgeConfig`.
/// Chosen as a memorable marker: 0xBAC0_CA1E ≈ "BACnet cable".
pub const MAGIC: u32 = 0xBAC0_CA1E;

/// Current schema version. Increment when fields are added/removed.
pub const CONFIG_VERSION: u16 = 5;

fn default_magic() -> u32 {
    MAGIC
}
fn default_version() -> u16 {
    CONFIG_VERSION
}

// ---------------------------------------------------------------------------
// NetworkConfig
// ---------------------------------------------------------------------------

/// Network / IP configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// If true, obtain IP via DHCP; otherwise use the static fields below.
    #[serde(default = "default_true")]
    pub dhcp: bool,
    /// Static IPv4 address (used when `dhcp` is false, or as DHCP fallback).
    #[serde(default = "default_ip")]
    pub ip: [u8; 4],
    /// Subnet mask.
    #[serde(default = "default_subnet")]
    pub subnet: [u8; 4],
    /// Default gateway.
    #[serde(default = "default_gateway")]
    pub gateway: [u8; 4],
    /// DNS server.
    #[serde(default = "default_dns")]
    pub dns: [u8; 4],
}

fn default_true() -> bool {
    true
}
fn default_ip() -> [u8; 4] {
    [192, 168, 1, 100]
}
fn default_subnet() -> [u8; 4] {
    [255, 255, 255, 0]
}
fn default_gateway() -> [u8; 4] {
    [192, 168, 1, 1]
}
fn default_dns() -> [u8; 4] {
    [8, 8, 8, 8]
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
    #[serde(default = "default_device_id")]
    pub device_id: u32,
    /// BACnet device name (Object_Name property of the Device object).
    #[serde(default = "default_device_name")]
    pub device_name: String<32>,
    /// MS/TP MAC address (0–127).
    #[serde(default = "default_mstp_mac")]
    pub mstp_mac: u8,
    /// MS/TP baud rate: 9600, 19200, 38400, or 76800.
    #[serde(default = "default_mstp_baud")]
    pub mstp_baud: u32,
    /// Max Master value for the MS/TP token-passing loop (0–127).
    #[serde(default = "default_max_master")]
    pub max_master: u8,
}

fn default_device_id() -> u32 {
    389_999
}
fn default_device_name() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("bacnet-bridge");
    s
}
fn default_mstp_mac() -> u8 {
    1
}
fn default_mstp_baud() -> u32 {
    19_200
}
fn default_max_master() -> u8 {
    127
}

impl Default for BacnetDeviceConfig {
    fn default() -> Self {
        Self {
            device_id: 389_999,
            device_name: default_device_name(),
            mstp_mac: 1,
            mstp_baud: 19_200,
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
    /// Intermediate role: read/write points, view config, no user management.
    Operator,
    /// Read-only access to device data and configuration.
    Viewer,
}

/// A single user account.
///
/// The password is stored as a salted SHA-256 hash split into two 32-byte fields:
/// - `password_salt` — unique random salt generated at account creation time.
/// - `password_hash` — SHA-256(`password_salt` || UTF-8 password bytes).
///
/// An all-zeros `password_hash` means the account is not yet configured / password
/// not set.  The `auth::verify_password` function enforces this invariant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserConfig {
    /// Login username (max 16 chars).
    pub username: String<16>,
    /// Per-user random salt (32 bytes).
    pub password_salt: [u8; 32],
    /// SHA-256(salt || password_utf8) — 32 bytes.
    pub password_hash: [u8; 32],
    /// Access role.
    pub role: UserRole,
}

// ---------------------------------------------------------------------------
// TokenConfig
// ---------------------------------------------------------------------------

/// A named API bearer token for programmatic access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    /// Human-readable name for this token (shown in the admin UI).
    pub name: String<32>,
    /// SHA-256 hash of the plaintext token bytes.
    pub token_hash: [u8; 32],
    /// Access role granted to requests bearing this token.
    pub role: UserRole,
    /// Username of the admin who created this token.
    pub created_by: String<16>,
}

// ---------------------------------------------------------------------------
// NtpConfig
// ---------------------------------------------------------------------------

/// NTP time synchronisation configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NtpConfig {
    /// Enable NTP synchronisation.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Use NTP servers from DHCP (option 42). If false or DHCP unavailable, use manual servers.
    #[serde(default = "default_true")]
    pub use_dhcp_servers: bool,
    /// Manual NTP server hostnames (resolved via DNS). Up to 3.
    #[serde(default = "default_ntp_servers")]
    pub servers: Vec<String<64>, 3>,
    /// Sync interval in seconds (default 3600, minimum 60).
    #[serde(default = "default_ntp_interval")]
    pub sync_interval_secs: u32,
}

fn default_ntp_servers() -> Vec<String<64>, 3> {
    let mut v = Vec::new();
    let mut s = String::new();
    let _ = s.push_str("pool.ntp.org");
    let _ = v.push(s);
    v
}
fn default_ntp_interval() -> u32 {
    3600
}

impl Default for NtpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_dhcp_servers: true,
            servers: default_ntp_servers(),
            sync_interval_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// SyslogConfig
// ---------------------------------------------------------------------------

/// Remote syslog (RFC 3164 / RFC 5424) configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyslogConfig {
    /// Enable remote syslog forwarding.
    #[serde(default)]
    pub enabled: bool,
    /// Syslog server hostname (resolved via DNS).
    #[serde(default)]
    pub server: String<64>,
    /// UDP port (default 514).
    #[serde(default = "default_syslog_port")]
    pub port: u16,
}

fn default_syslog_port() -> u16 {
    514
}

impl Default for SyslogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            port: 514,
        }
    }
}

// ---------------------------------------------------------------------------
// MqttConfig
// ---------------------------------------------------------------------------

/// MQTT broker and publishing configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MqttConfig {
    /// Enable MQTT publishing.
    #[serde(default)]
    pub enabled: bool,
    /// MQTT broker hostname.
    #[serde(default)]
    pub broker: String<64>,
    /// TCP port (default 1883).
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    /// MQTT client ID.
    #[serde(default = "default_mqtt_client_id")]
    pub client_id: String<32>,
    /// Optional username (empty = anonymous).
    #[serde(default)]
    pub username: String<32>,
    /// Optional password (empty = none).
    #[serde(default)]
    pub password: String<32>,
    /// Topic prefix for publishing point values (e.g. "bacnet-bridge").
    #[serde(default = "default_mqtt_topic_prefix")]
    pub topic_prefix: String<32>,
    /// Enable Home Assistant MQTT auto-discovery.
    #[serde(default)]
    pub ha_discovery_enabled: bool,
    /// HA discovery prefix (default "homeassistant").
    #[serde(default = "default_ha_prefix")]
    pub ha_discovery_prefix: String<32>,
    /// Which points to publish, in "objectType:instance" format. Empty = publish all.
    #[serde(default)]
    pub publish_points: Vec<String<32>, 64>,
    /// Enable TLS for MQTT connection (port 8883).
    #[serde(default)]
    pub tls_enabled: bool,
}

fn default_mqtt_port() -> u16 {
    1883
}
fn default_mqtt_client_id() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("bacnet-bridge");
    s
}
fn default_mqtt_topic_prefix() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("bacnet-bridge");
    s
}
fn default_ha_prefix() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("homeassistant");
    s
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            broker: String::new(),
            port: 1883,
            client_id: default_mqtt_client_id(),
            username: String::new(),
            password: String::new(),
            topic_prefix: default_mqtt_topic_prefix(),
            ha_discovery_enabled: false,
            ha_discovery_prefix: default_ha_prefix(),
            publish_points: Vec::new(),
            tls_enabled: false,
        }
    }
}

// ---------------------------------------------------------------------------
// SnmpConfig
// ---------------------------------------------------------------------------

/// SNMP agent configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnmpConfig {
    /// Enable SNMP agent.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// SNMPv1/v2c community string.
    #[serde(default = "default_snmp_community")]
    pub community: String<32>,
}

fn default_snmp_community() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("public");
    s
}

impl Default for SnmpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            community: default_snmp_community(),
        }
    }
}

// ---------------------------------------------------------------------------
// TlsConfig
// ---------------------------------------------------------------------------

/// TLS / HTTPS server configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TlsConfig {
    /// Enable HTTPS server (not yet implemented; reserved for future use).
    #[serde(default)]
    pub server_enabled: bool,
    /// HTTPS port (default 443).
    #[serde(default = "default_https_port")]
    pub https_port: u16,
}

fn default_https_port() -> u16 {
    443
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            server_enabled: false,
            https_port: 443,
        }
    }
}

// ---------------------------------------------------------------------------
// OtaConfig
// ---------------------------------------------------------------------------

/// Over-the-air firmware update configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OtaConfig {
    /// Automatically check for and apply firmware updates.
    #[serde(default)]
    pub auto_update: bool,
    /// URL to the firmware manifest JSON file.
    #[serde(default)]
    pub manifest_url: String<128>,
    /// Update channel — "release" or "beta".
    #[serde(default = "default_ota_channel")]
    pub channel: String<16>,
    /// How often to check for updates (seconds). Default 3600.
    #[serde(default = "default_ota_interval")]
    pub check_interval_secs: u32,
}

fn default_ota_channel() -> String<16> {
    let mut s = String::new();
    let _ = s.push_str("release");
    s
}
fn default_ota_interval() -> u32 {
    3600
}

impl Default for OtaConfig {
    fn default() -> Self {
        Self {
            auto_update: false,
            manifest_url: String::new(),
            channel: default_ota_channel(),
            check_interval_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// PointRule — replaces PointConfig
// ---------------------------------------------------------------------------

/// How a discovered BACnet point should be treated by the bridge.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum PointMode {
    /// Suppress the point — do not forward, publish, or display it.
    Ignore,
    /// Forward the raw value with no transformation.
    #[default]
    Passthrough,
    /// Apply the `processors` pipeline before forwarding.
    Processed,
}

/// A single processing step applied to a point value.
///
/// # Memory budget note
///
/// `Processor` is stored inline in `Vec<Processor, 4>` inside each `PointRule`.
/// The largest variant is `MapStates` (up to 8 state labels of ≤ 12 chars each).
/// Keeping labels short caps `Processor` at ~110 bytes, `Vec<Processor, 4>` at ~450 bytes,
/// and `PointRule` at ~480 bytes — essential for fitting the point table in SRAM.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Processor {
    /// Override the engineering unit code (metadata only — does not change the value).
    SetUnit(u16),
    /// Linear scale: `display = raw * factor + offset`.
    Scale { factor: f32, offset: f32 },
    /// Map multi-state integer indices to human-readable labels.
    ///
    /// Up to 8 states with labels up to 12 characters each. Index 0 = BACnet state 1.
    MapStates(Vec<String<12>, 8>),
}

/// A named, reusable pipeline of [`Processor`] steps.
///
/// Convertors are stored globally in [`BridgeConfig::convertors`] (max 16) and
/// referenced by ID from [`PointRule::convertor_id`].  Keeping the pipeline in a
/// shared table avoids duplicating processor lists across every point rule.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Convertor {
    /// Short identifier used to link a [`PointRule`] to this convertor (max 16 chars).
    pub id: String<16>,
    /// Human-readable name shown in the admin UI (max 32 chars).
    pub name: String<32>,
    /// Ordered processing steps applied forward (BACnet → display) and
    /// in reverse (display → BACnet) when writing.
    #[serde(default)]
    pub processors: Vec<Processor, 4>,
}

/// Per-point processing and routing rule.
///
/// Each `PointRule` targets one BACnet object (identified by device ID + object type +
/// instance) and specifies how the bridge should handle its value.
///
/// Exposure is now binary: a point is either active (Passthrough or Processed) or
/// ignored (`Ignore` mode). Per-channel exposure flags have been removed.
///
/// # Memory budget
///
/// `PointRule` is stored in a `Vec<PointRule, 64>` inline array in `BridgeConfig`.
/// With `convertor_id` (16 bytes) replacing the inline processor vec (~450 bytes)
/// and `ExposureConfig` (~4 bytes), each rule shrinks to ~80 bytes.
/// 64 rules × ~80 bytes = ~5 KB — well within the SRAM budget.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PointRule {
    /// BACnet device instance number of the owning device.
    pub device_id: u32,
    /// BACnet object type code.
    pub object_type: u16,
    /// BACnet object instance number.
    pub object_instance: u32,
    /// How the bridge should treat this point.
    #[serde(default)]
    pub mode: PointMode,
    /// ID of the [`Convertor`] to apply when `mode` is [`PointMode::Processed`].
    /// Empty string means no convertor (acts like Passthrough).
    #[serde(default)]
    pub convertor_id: String<16>,
}

// ---------------------------------------------------------------------------
// BridgeConfig — v4
// ---------------------------------------------------------------------------

fn default_hostname() -> String<32> {
    let mut s = String::new();
    let _ = s.push_str("bacnet-bridge");
    s
}

/// Top-level bridge configuration struct, persisted to flash.
///
/// All fields carry `#[serde(default)]` so that a stored config from an older
/// firmware version will deserialise without error — missing fields get their
/// `Default` values automatically (forward compatibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Must equal `MAGIC` (0xBAC0_CA1E) for the config to be considered valid.
    #[serde(default = "default_magic")]
    pub magic: u32,
    /// Schema version. Must equal `CONFIG_VERSION`.
    #[serde(default = "default_version")]
    pub version: u16,
    /// True once the admin has completed first-boot setup.
    #[serde(default)]
    pub provisioned: bool,
    /// Network / IP settings.
    #[serde(default)]
    pub network: NetworkConfig,
    /// mDNS hostname (advertised as `{hostname}.local`).
    #[serde(default = "default_hostname")]
    pub hostname: String<32>,
    /// BACnet device and MS/TP settings.
    #[serde(default)]
    pub bacnet: BacnetDeviceConfig,
    /// NTP time synchronisation settings.
    #[serde(default)]
    pub ntp: NtpConfig,
    /// Remote syslog settings.
    #[serde(default)]
    pub syslog: SyslogConfig,
    /// MQTT broker and publishing settings.
    #[serde(default)]
    pub mqtt: MqttConfig,
    /// SNMP agent settings.
    #[serde(default)]
    pub snmp: SnmpConfig,
    /// TLS / HTTPS server settings.
    #[serde(default)]
    pub tls: TlsConfig,
    /// OTA firmware update settings.
    #[serde(default)]
    pub ota: OtaConfig,
    /// Configured user accounts (max 8).
    #[serde(default)]
    pub users: Vec<UserConfig, 8>,
    /// Named API bearer tokens (max 16).
    #[serde(default)]
    pub tokens: Vec<TokenConfig, 16>,
    /// Named value convertors referenced by point rules (max 16).
    #[serde(default)]
    pub convertors: Vec<Convertor, 16>,
    /// Per-point processing rules (max 64).
    ///
    /// Points without explicit rules use passthrough defaults implicitly.
    #[serde(default)]
    pub points: Vec<PointRule, 64>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            magic: MAGIC,
            version: CONFIG_VERSION,
            provisioned: false,
            network: NetworkConfig::default(),
            hostname: default_hostname(),
            bacnet: BacnetDeviceConfig::default(),
            ntp: NtpConfig::default(),
            syslog: SyslogConfig::default(),
            mqtt: MqttConfig::default(),
            snmp: SnmpConfig::default(),
            tls: TlsConfig::default(),
            ota: OtaConfig::default(),
            users: Vec::new(),
            tokens: Vec::new(),
            convertors: Vec::new(),
            points: Vec::new(),
        }
    }
}

/// Valid MS/TP baud rates (per BACnet clause 9.3). 0 = auto-detect.
const VALID_BAUD_RATES: [u32; 5] = [0, 9600, 19200, 38400, 76800];

/// Maximum BACnet device instance number (22-bit field, ASHRAE 135 clause 12.11).
pub const DEVICE_ID_MAX: u32 = 0x003F_FFFE;

impl BridgeConfig {
    /// Return true if the magic number, version, and all semantic fields are valid.
    ///
    /// Checks:
    /// - `magic` == `MAGIC`
    /// - `version` == `CONFIG_VERSION`
    /// - `bacnet.mstp_mac` <= 127
    /// - `bacnet.mstp_baud` is one of {0 (auto), 9600, 19200, 38400, 76800}
    /// - `bacnet.device_id` <= 0x003F_FFFE (22-bit BACnet instance max)
    /// - `bacnet.max_master` >= 1 && <= 127
    /// - `hostname` is non-empty
    /// - `ntp.sync_interval_secs` >= 60
    /// - `syslog.port` > 0 and `syslog.server` non-empty when `syslog.enabled`
    /// - `mqtt.port` > 0 and `mqtt.broker` non-empty when `mqtt.enabled`
    /// - `snmp.community` non-empty when `snmp.enabled`
    pub fn validate(&self) -> bool {
        if self.magic != MAGIC || self.version != CONFIG_VERSION {
            return false;
        }
        if self.bacnet.mstp_mac > 127 {
            return false;
        }
        if !VALID_BAUD_RATES.contains(&self.bacnet.mstp_baud) {
            return false;
        }
        if self.bacnet.device_id > DEVICE_ID_MAX {
            return false;
        }
        if self.bacnet.max_master < 1 || self.bacnet.max_master > 127 {
            return false;
        }
        if self.hostname.is_empty() {
            return false;
        }
        if self.ntp.sync_interval_secs < 60 {
            return false;
        }
        if self.syslog.enabled {
            if self.syslog.port == 0 {
                return false;
            }
            if self.syslog.server.is_empty() {
                return false;
            }
        }
        if self.mqtt.enabled {
            if self.mqtt.port == 0 {
                return false;
            }
            if self.mqtt.broker.is_empty() {
                return false;
            }
        }
        if self.snmp.enabled && self.snmp.community.is_empty() {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Basic constants and defaults
    // -----------------------------------------------------------------------

    #[test]
    fn magic_constant() {
        assert_eq!(MAGIC, 0xBAC0_CA1E);
    }

    #[test]
    fn config_version_is_5() {
        assert_eq!(CONFIG_VERSION, 5);
    }

    #[test]
    fn default_validates() {
        let cfg = BridgeConfig::default();
        assert!(cfg.validate());
    }

    #[test]
    fn default_not_provisioned() {
        let cfg = BridgeConfig::default();
        assert!(!cfg.provisioned);
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
        assert_eq!(cfg.bacnet.mstp_baud, 19_200);
        assert_eq!(cfg.bacnet.mstp_mac, 1);
        assert_eq!(cfg.bacnet.max_master, 127);
    }

    #[test]
    fn default_hostname() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.hostname.as_str(), "bacnet-bridge");
    }

    // -----------------------------------------------------------------------
    // Validation edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_mstp_mac_128_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_mac = 128;
        assert!(!cfg.validate(), "mstp_mac=128 should fail validation");
    }

    #[test]
    fn test_validate_bad_baud_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_baud = 115200;
        assert!(!cfg.validate(), "mstp_baud=115200 should fail validation");
    }

    #[test]
    fn test_validate_device_id_too_large_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.device_id = DEVICE_ID_MAX + 1;
        assert!(!cfg.validate(), "device_id > max should fail validation");
    }

    #[test]
    fn test_validate_max_master_zero_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.max_master = 0;
        assert!(!cfg.validate(), "max_master=0 should fail validation");
    }

    #[test]
    fn test_validate_empty_hostname_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.hostname = heapless::String::new();
        assert!(!cfg.validate(), "empty hostname should fail validation");
    }

    #[test]
    fn test_validate_all_good_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_mac = 1;
        cfg.bacnet.mstp_baud = 9600;
        cfg.bacnet.device_id = 100;
        cfg.bacnet.max_master = 127;
        assert!(cfg.validate(), "fully valid config should pass");
    }

    #[test]
    fn test_validate_device_id_at_max_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.device_id = DEVICE_ID_MAX;
        assert!(cfg.validate(), "device_id == DEVICE_ID_MAX should pass");
    }

    #[test]
    fn test_validate_max_master_128_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.max_master = 128;
        assert!(!cfg.validate(), "max_master=128 should fail validation");
    }

    // -----------------------------------------------------------------------
    // Serialise / deserialise round trips
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_deserialize_round_trip() {
        let cfg = BridgeConfig::default();
        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
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
                password_salt: [0xCDu8; 32],
                password_hash: [0xABu8; 32],
                role: UserRole::Admin,
            })
            .expect("push failed");

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.users.len(), 1);
        assert_eq!(decoded.users[0].role, UserRole::Admin);
    }

    #[test]
    fn network_config_static_ip() {
        let mut cfg = BridgeConfig::default();
        cfg.network.dhcp = false;
        cfg.network.ip = [10, 0, 0, 50];
        cfg.network.subnet = [255, 255, 0, 0];

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(!decoded.network.dhcp);
        assert_eq!(decoded.network.ip, [10, 0, 0, 50]);
        assert_eq!(decoded.network.subnet, [255, 255, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // UserRole — Operator
    // -----------------------------------------------------------------------

    #[test]
    fn user_role_operator_round_trip() {
        let mut cfg = BridgeConfig::default();
        let mut username = String::new();
        let _ = username.push_str("ops");
        cfg.users
            .push(UserConfig {
                username,
                password_salt: [0u8; 32],
                password_hash: [0u8; 32],
                role: UserRole::Operator,
            })
            .unwrap();

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert_eq!(decoded.users[0].role, UserRole::Operator);
    }

    #[test]
    fn all_three_user_roles_distinct() {
        assert_ne!(UserRole::Admin, UserRole::Operator);
        assert_ne!(UserRole::Admin, UserRole::Viewer);
        assert_ne!(UserRole::Operator, UserRole::Viewer);
    }

    // -----------------------------------------------------------------------
    // TokenConfig
    // -----------------------------------------------------------------------

    #[test]
    fn token_config_round_trip() {
        let mut cfg = BridgeConfig::default();
        let mut name = String::new();
        let _ = name.push_str("ci-token");
        let mut created_by = String::new();
        let _ = created_by.push_str("admin");
        cfg.tokens
            .push(TokenConfig {
                name,
                token_hash: [0xFFu8; 32],
                role: UserRole::Viewer,
                created_by,
            })
            .unwrap();

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert_eq!(decoded.tokens.len(), 1);
        assert_eq!(decoded.tokens[0].role, UserRole::Viewer);
        assert_eq!(decoded.tokens[0].token_hash, [0xFFu8; 32]);
    }

    // -----------------------------------------------------------------------
    // TlsConfig
    // -----------------------------------------------------------------------

    #[test]
    fn default_tls_config() {
        let cfg = BridgeConfig::default();
        assert!(!cfg.tls.server_enabled);
        assert_eq!(cfg.tls.https_port, 443);
    }

    #[test]
    fn tls_config_round_trip() {
        let mut cfg = BridgeConfig::default();
        cfg.tls.server_enabled = true;
        cfg.tls.https_port = 8443;

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert!(decoded.tls.server_enabled);
        assert_eq!(decoded.tls.https_port, 8443);
    }

    // -----------------------------------------------------------------------
    // OtaConfig
    // -----------------------------------------------------------------------

    #[test]
    fn default_ota_config() {
        let cfg = BridgeConfig::default();
        assert!(!cfg.ota.auto_update);
        assert!(cfg.ota.manifest_url.is_empty());
        assert_eq!(cfg.ota.channel.as_str(), "release");
        assert_eq!(cfg.ota.check_interval_secs, 3600);
    }

    #[test]
    fn ota_config_round_trip() {
        let mut cfg = BridgeConfig::default();
        cfg.ota.auto_update = true;
        let mut url = String::new();
        let _ = url.push_str("https://example.com/fw.json");
        cfg.ota.manifest_url = url;
        let mut channel = String::new();
        let _ = channel.push_str("beta");
        cfg.ota.channel = channel;
        cfg.ota.check_interval_secs = 7200;

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert!(decoded.ota.auto_update);
        assert_eq!(
            decoded.ota.manifest_url.as_str(),
            "https://example.com/fw.json"
        );
        assert_eq!(decoded.ota.channel.as_str(), "beta");
        assert_eq!(decoded.ota.check_interval_secs, 7200);
    }

    // -----------------------------------------------------------------------
    // PointRule / Convertor
    // -----------------------------------------------------------------------

    #[test]
    fn point_rule_default_mode_is_passthrough() {
        let mode = PointMode::default();
        assert_eq!(mode, PointMode::Passthrough);
    }

    #[test]
    fn point_rule_round_trip_passthrough() {
        let mut cfg = BridgeConfig::default();
        cfg.points
            .push(PointRule {
                device_id: 1000,
                object_type: 0,
                object_instance: 42,
                mode: PointMode::Passthrough,
                convertor_id: String::new(),
            })
            .unwrap();

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert_eq!(decoded.points.len(), 1);
        assert_eq!(decoded.points[0].device_id, 1000);
        assert_eq!(decoded.points[0].object_instance, 42);
        assert_eq!(decoded.points[0].mode, PointMode::Passthrough);
    }

    #[test]
    fn point_rule_ignore_mode() {
        let rule = PointRule {
            device_id: 5,
            object_type: 3,
            object_instance: 1,
            mode: PointMode::Ignore,
            convertor_id: String::new(),
        };
        assert_eq!(rule.mode, PointMode::Ignore);
    }

    #[test]
    fn point_rule_processed_with_convertor_id() {
        let mut id: String<16> = String::new();
        let _ = id.push_str("temp-c");

        let rule = PointRule {
            device_id: 100,
            object_type: 0,
            object_instance: 1,
            mode: PointMode::Processed,
            convertor_id: id,
        };
        assert_eq!(rule.mode, PointMode::Processed);
        assert_eq!(rule.convertor_id.as_str(), "temp-c");
    }

    #[test]
    fn convertor_round_trip() {
        let mut cfg = BridgeConfig::default();

        let mut c_id: String<16> = String::new();
        let _ = c_id.push_str("temp-c");
        let mut c_name: String<32> = String::new();
        let _ = c_name.push_str("Temperature (C)");
        let mut processors: Vec<Processor, 4> = Vec::new();
        processors.push(Processor::SetUnit(62)).unwrap();
        cfg.convertors
            .push(Convertor {
                id: c_id,
                name: c_name,
                processors,
            })
            .unwrap();

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert_eq!(decoded.convertors.len(), 1);
        assert_eq!(decoded.convertors[0].id.as_str(), "temp-c");
        assert_eq!(decoded.convertors[0].processors.len(), 1);
    }

    #[test]
    fn point_rule_with_set_unit_processor() {
        let mut processors: Vec<Processor, 4> = Vec::new();
        processors.push(Processor::SetUnit(0)).unwrap(); // 0 = DegreesCelsius
        assert_eq!(processors.len(), 1);
    }

    #[test]
    fn point_rule_with_map_states_processor() {
        let mut states: Vec<String<12>, 8> = Vec::new();
        let mut s1 = String::new();
        let _ = s1.push_str("Off");
        let mut s2 = String::new();
        let _ = s2.push_str("On");
        states.push(s1).unwrap();
        states.push(s2).unwrap();

        let mut processors: Vec<Processor, 4> = Vec::new();
        processors.push(Processor::MapStates(states)).unwrap();
        assert_eq!(processors.len(), 1);
    }

    // -----------------------------------------------------------------------
    // NtpConfig tests (preserved)
    // -----------------------------------------------------------------------

    #[test]
    fn default_ntp_config() {
        let cfg = BridgeConfig::default();
        assert!(cfg.ntp.enabled);
        assert!(cfg.ntp.use_dhcp_servers);
        assert_eq!(cfg.ntp.servers.len(), 1);
        assert_eq!(cfg.ntp.servers[0].as_str(), "pool.ntp.org");
        assert_eq!(cfg.ntp.sync_interval_secs, 3600);
    }

    #[test]
    fn ntp_sync_interval_too_short_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.ntp.sync_interval_secs = 59;
        assert!(!cfg.validate(), "sync_interval_secs < 60 should fail");
    }

    #[test]
    fn ntp_sync_interval_exactly_60_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.ntp.sync_interval_secs = 60;
        assert!(cfg.validate(), "sync_interval_secs == 60 should pass");
    }

    #[test]
    fn ntp_sync_interval_zero_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.ntp.sync_interval_secs = 0;
        assert!(!cfg.validate(), "sync_interval_secs == 0 should fail");
    }

    #[test]
    fn ntp_round_trip() {
        let cfg = BridgeConfig::default();
        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.ntp.enabled, cfg.ntp.enabled);
        assert_eq!(decoded.ntp.sync_interval_secs, cfg.ntp.sync_interval_secs);
        assert_eq!(decoded.ntp.servers[0].as_str(), "pool.ntp.org");
    }

    // -----------------------------------------------------------------------
    // SyslogConfig tests (preserved)
    // -----------------------------------------------------------------------

    #[test]
    fn default_syslog_config() {
        let cfg = BridgeConfig::default();
        assert!(!cfg.syslog.enabled);
        assert!(cfg.syslog.server.is_empty());
        assert_eq!(cfg.syslog.port, 514);
    }

    #[test]
    fn syslog_enabled_zero_port_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = true;
        cfg.syslog.port = 0;
        let mut server = String::new();
        let _ = server.push_str("logs.example.com");
        cfg.syslog.server = server;
        assert!(!cfg.validate(), "syslog enabled with port=0 should fail");
    }

    #[test]
    fn syslog_enabled_valid_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = true;
        let mut server = String::new();
        let _ = server.push_str("logs.example.com");
        cfg.syslog.server = server;
        assert!(cfg.validate(), "syslog enabled with valid port should pass");
    }

    #[test]
    fn syslog_disabled_zero_port_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = false;
        cfg.syslog.port = 0;
        assert!(cfg.validate(), "syslog disabled with port=0 should pass");
    }

    #[test]
    fn syslog_enabled_empty_server_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = true;
        cfg.syslog.server = String::new();
        cfg.syslog.port = 514;
        assert!(
            !cfg.validate(),
            "syslog enabled with empty server must fail"
        );
    }

    #[test]
    fn syslog_round_trip() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = true;
        let mut server = String::new();
        let _ = server.push_str("syslog.local");
        cfg.syslog.server = server;
        cfg.syslog.port = 514;

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(decoded.syslog.enabled);
        assert_eq!(decoded.syslog.server.as_str(), "syslog.local");
        assert_eq!(decoded.syslog.port, 514);
    }

    // -----------------------------------------------------------------------
    // MqttConfig tests (preserved)
    // -----------------------------------------------------------------------

    #[test]
    fn default_mqtt_config() {
        let cfg = BridgeConfig::default();
        assert!(!cfg.mqtt.enabled);
        assert!(cfg.mqtt.broker.is_empty());
        assert_eq!(cfg.mqtt.port, 1883);
        assert_eq!(cfg.mqtt.client_id.as_str(), "bacnet-bridge");
        assert_eq!(cfg.mqtt.topic_prefix.as_str(), "bacnet-bridge");
        assert!(!cfg.mqtt.ha_discovery_enabled);
        assert_eq!(cfg.mqtt.ha_discovery_prefix.as_str(), "homeassistant");
        assert!(cfg.mqtt.publish_points.is_empty());
    }

    #[test]
    fn mqtt_enabled_empty_broker_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = true;
        assert!(
            !cfg.validate(),
            "mqtt enabled with empty broker should fail"
        );
    }

    #[test]
    fn mqtt_enabled_zero_port_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = true;
        cfg.mqtt.port = 0;
        let mut broker = String::new();
        let _ = broker.push_str("mqtt.example.com");
        cfg.mqtt.broker = broker;
        assert!(!cfg.validate(), "mqtt enabled with port=0 should fail");
    }

    #[test]
    fn mqtt_enabled_valid_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = true;
        let mut broker = String::new();
        let _ = broker.push_str("mqtt.example.com");
        cfg.mqtt.broker = broker;
        assert!(
            cfg.validate(),
            "mqtt enabled with broker and port should pass"
        );
    }

    #[test]
    fn mqtt_disabled_empty_broker_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = false;
        assert!(
            cfg.validate(),
            "mqtt disabled with empty broker should pass"
        );
    }

    #[test]
    fn mqtt_round_trip() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = true;
        let mut broker = String::new();
        let _ = broker.push_str("broker.local");
        cfg.mqtt.broker = broker;
        cfg.mqtt.ha_discovery_enabled = true;

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(decoded.mqtt.enabled);
        assert_eq!(decoded.mqtt.broker.as_str(), "broker.local");
        assert_eq!(decoded.mqtt.port, 1883);
        assert!(decoded.mqtt.ha_discovery_enabled);
        assert_eq!(decoded.mqtt.ha_discovery_prefix.as_str(), "homeassistant");
    }

    // -----------------------------------------------------------------------
    // SnmpConfig tests (preserved)
    // -----------------------------------------------------------------------

    #[test]
    fn default_snmp_config() {
        let cfg = BridgeConfig::default();
        assert!(cfg.snmp.enabled);
        assert_eq!(cfg.snmp.community.as_str(), "public");
    }

    #[test]
    fn snmp_enabled_empty_community_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.snmp.community = String::new();
        assert!(
            !cfg.validate(),
            "snmp enabled with empty community should fail"
        );
    }

    #[test]
    fn snmp_disabled_empty_community_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.snmp.enabled = false;
        cfg.snmp.community = String::new();
        assert!(
            cfg.validate(),
            "snmp disabled with empty community should pass"
        );
    }

    #[test]
    fn snmp_custom_community_passes() {
        let mut cfg = BridgeConfig::default();
        let mut community = String::new();
        let _ = community.push_str("private");
        cfg.snmp.community = community;
        assert!(
            cfg.validate(),
            "snmp with custom community string should pass"
        );
    }

    #[test]
    fn snmp_round_trip() {
        let cfg = BridgeConfig::default();
        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.snmp.enabled, cfg.snmp.enabled);
        assert_eq!(decoded.snmp.community.as_str(), "public");
    }

    // -----------------------------------------------------------------------
    // Multiple points in one config
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_point_rules() {
        let mut cfg = BridgeConfig::default();
        for i in 0u32..5 {
            cfg.points
                .push(PointRule {
                    device_id: 1000,
                    object_type: 0,
                    object_instance: i,
                    mode: PointMode::Passthrough,
                    convertor_id: String::new(),
                })
                .unwrap();
        }
        assert_eq!(cfg.points.len(), 5);

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert_eq!(decoded.points.len(), 5);
        for i in 0..5 {
            assert_eq!(decoded.points[i].object_instance, i as u32);
        }
    }

    // -----------------------------------------------------------------------
    // users Vec capacity: v4 supports 8 (was 4)
    // -----------------------------------------------------------------------

    #[test]
    fn can_store_eight_users() {
        let mut cfg = BridgeConfig::default();
        for i in 0u8..8 {
            let mut username = String::new();
            let _ = username.push(char::from(b'a' + i));
            cfg.users
                .push(UserConfig {
                    username,
                    password_salt: [0u8; 32],
                    password_hash: [0u8; 32],
                    role: UserRole::Viewer,
                })
                .expect("push should fit 8 users");
        }
        assert_eq!(cfg.users.len(), 8);
    }

    // -----------------------------------------------------------------------
    // provisioned flag round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn provisioned_flag_round_trip() {
        let mut cfg = BridgeConfig::default();
        cfg.provisioned = true;

        let mut buf = [0u8; 8192];
        let json = serde_json_core::to_slice(&cfg, &mut buf).unwrap();
        let (decoded, _): (BridgeConfig, _) = serde_json_core::from_slice(&buf[..json]).unwrap();
        assert!(decoded.provisioned);
    }

    // -----------------------------------------------------------------------
    // Serde edge cases — malformed / partial JSON must not panic
    // -----------------------------------------------------------------------

    /// Completely empty input must return an error, not panic.
    #[test]
    fn deserialize_empty_input_is_error() {
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(b"");
        assert!(
            result.is_err(),
            "empty input must be a deserialization error"
        );
    }

    /// Truncated JSON object (no closing brace) must return an error.
    #[test]
    fn deserialize_truncated_json_is_error() {
        let result: Result<(BridgeConfig, _), _> =
            serde_json_core::from_slice(b"{\"magic\":3134825022,\"version\":4");
        assert!(
            result.is_err(),
            "truncated JSON must be a deserialization error"
        );
    }

    /// A JSON null where an object is expected must return an error.
    #[test]
    fn deserialize_null_is_error() {
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(b"null");
        assert!(
            result.is_err(),
            "null input must be a deserialization error"
        );
    }

    /// A JSON array where an object is expected must return an error.
    #[test]
    fn deserialize_array_is_error() {
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(b"[]");
        assert!(
            result.is_err(),
            "array input must be a deserialization error"
        );
    }

    /// A bare integer where an object is expected must return an error.
    #[test]
    fn deserialize_integer_is_error() {
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(b"42");
        assert!(
            result.is_err(),
            "bare integer must be a deserialization error"
        );
    }

    /// An empty JSON object `{}` must succeed and produce default values
    /// (all fields carry `#[serde(default)]`).
    #[test]
    fn deserialize_empty_object_uses_defaults() {
        let (cfg, _): (BridgeConfig, _) =
            serde_json_core::from_slice(b"{}").expect("empty object must deserialize to defaults");
        // Magic and version come from their default fns.
        assert_eq!(cfg.magic, MAGIC);
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert_eq!(cfg.hostname.as_str(), "bacnet-bridge");
        assert!(cfg.network.dhcp);
    }

    /// Unknown / extra fields in the JSON must be silently ignored (forward
    /// compatibility for newer firmware writing to older flash).
    #[test]
    fn deserialize_unknown_fields_ignored() {
        let json =
            br#"{"magic":3134825022,"version":4,"unknown_future_field":true,"provisioned":true}"#;
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(json);
        assert!(
            result.is_ok(),
            "unknown fields must be silently ignored; got: {:?}",
            result.err()
        );
        let (cfg, _) = result.unwrap();
        assert!(cfg.provisioned);
    }

    /// A wrong type for a known field (string instead of bool) must return an
    /// error rather than panic or silently corrupt the struct.
    #[test]
    fn deserialize_wrong_type_for_field_is_error() {
        // `provisioned` expects a bool; supply a string.
        let json = br#"{"magic":3134825022,"version":4,"provisioned":"yes"}"#;
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(json);
        assert!(
            result.is_err(),
            "wrong type for 'provisioned' must be a deserialization error"
        );
    }

    /// An integer that overflows u32 in the `magic` field must return an error.
    #[test]
    fn deserialize_magic_overflow_is_error() {
        // 9999999999 exceeds u32::MAX (4294967295).
        let json = br#"{"magic":9999999999}"#;
        let result: Result<(BridgeConfig, _), _> = serde_json_core::from_slice(json);
        assert!(
            result.is_err(),
            "u32 overflow in 'magic' must be a deserialization error"
        );
    }

    /// A partial but syntactically valid single-field object must succeed and
    /// produce correct defaults for all other fields.
    #[test]
    fn deserialize_partial_object_uses_defaults_for_missing_fields() {
        let json = br#"{"provisioned":true}"#;
        let (cfg, _): (BridgeConfig, _) = serde_json_core::from_slice(json)
            .expect("partial object with valid field must deserialize");
        assert!(cfg.provisioned);
        // All other fields should be defaults.
        assert_eq!(cfg.magic, MAGIC);
        assert_eq!(cfg.hostname.as_str(), "bacnet-bridge");
        assert!(cfg.users.is_empty());
    }

    /// A magic field set to 0 survives deserialization but fails `validate()`.
    #[test]
    fn deserialize_bad_magic_survives_but_fails_validate() {
        let json = br#"{"magic":0,"version":5,"provisioned":false}"#;
        let (cfg, _): (BridgeConfig, _) =
            serde_json_core::from_slice(json).expect("bad magic must deserialize without panic");
        assert!(!cfg.validate(), "bad magic must fail validate()");
    }

    /// A version field with the wrong value survives deserialization but fails `validate()`.
    #[test]
    fn deserialize_bad_version_survives_but_fails_validate() {
        let json = br#"{"magic":3134825022,"version":99}"#;
        let (cfg, _): (BridgeConfig, _) =
            serde_json_core::from_slice(json).expect("bad version must deserialize without panic");
        assert!(!cfg.validate(), "bad version must fail validate()");
    }
}
