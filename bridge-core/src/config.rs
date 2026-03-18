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
pub const CONFIG_VERSION: u16 = 2;

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
// NtpConfig
// ---------------------------------------------------------------------------

/// NTP time synchronisation configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NtpConfig {
    /// Enable NTP synchronisation.
    pub enabled: bool,
    /// Use NTP servers from DHCP (option 42). If false or DHCP unavailable, use manual servers.
    pub use_dhcp_servers: bool,
    /// Manual NTP server hostnames (resolved via DNS). Up to 3.
    pub servers: Vec<String<64>, 3>,
    /// Sync interval in seconds (default 3600, minimum 60).
    pub sync_interval_secs: u32,
}

impl Default for NtpConfig {
    fn default() -> Self {
        let mut servers = Vec::new();
        let mut s = String::new();
        let _ = s.push_str("pool.ntp.org");
        let _ = servers.push(s);
        Self {
            enabled: true,
            use_dhcp_servers: true,
            servers,
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
    pub enabled: bool,
    /// Syslog server hostname (resolved via DNS).
    pub server: String<64>,
    /// UDP port (default 514).
    pub port: u16,
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
    pub enabled: bool,
    /// MQTT broker hostname.
    pub broker: String<64>,
    /// TCP port (default 1883).
    pub port: u16,
    /// MQTT client ID.
    pub client_id: String<32>,
    /// Optional username (empty = anonymous).
    pub username: String<32>,
    /// Optional password (empty = none).
    pub password: String<32>,
    /// Topic prefix for publishing point values (e.g. "bacnet-bridge").
    pub topic_prefix: String<32>,
    /// Enable Home Assistant MQTT auto-discovery.
    pub ha_discovery_enabled: bool,
    /// HA discovery prefix (default "homeassistant").
    pub ha_discovery_prefix: String<32>,
    /// Which points to publish, in "objectType:instance" format. Empty = publish all.
    pub publish_points: Vec<String<32>, 64>,
    /// Enable TLS for MQTT connection (port 8883).
    ///
    /// When true the task wraps the TCP socket with `embedded-tls` before
    /// sending the MQTT CONNECT packet.  Certificate verification is currently
    /// skipped (`UnsecureProvider`); a CA cert upload path is a future TODO.
    ///
    /// Note: if `tls_enabled` is true and `port` is still 1883 the connection
    /// will succeed at the TCP level but the broker will likely reject it because
    /// it expects plain MQTT on that port.  Consider changing `port` to 8883.
    pub tls_enabled: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        let mut client_id = String::new();
        let _ = client_id.push_str("bacnet-bridge");
        let mut topic_prefix = String::new();
        let _ = topic_prefix.push_str("bacnet-bridge");
        let mut ha_discovery_prefix = String::new();
        let _ = ha_discovery_prefix.push_str("homeassistant");
        Self {
            enabled: false,
            broker: String::new(),
            port: 1883,
            client_id,
            username: String::new(),
            password: String::new(),
            topic_prefix,
            ha_discovery_enabled: false,
            ha_discovery_prefix,
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
    pub enabled: bool,
    /// SNMPv1/v2c community string.
    pub community: String<32>,
}

impl Default for SnmpConfig {
    fn default() -> Self {
        let mut community = String::new();
        let _ = community.push_str("public");
        Self {
            enabled: true,
            community,
        }
    }
}

// ---------------------------------------------------------------------------
// BridgeConfig
// ---------------------------------------------------------------------------

/// Top-level bridge configuration struct, persisted to flash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Must equal `MAGIC` (0xBAC0_CA1E) for the config to be considered valid.
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
    /// NTP time synchronisation settings.
    pub ntp: NtpConfig,
    /// Remote syslog settings.
    pub syslog: SyslogConfig,
    /// MQTT broker and publishing settings.
    pub mqtt: MqttConfig,
    /// SNMP agent settings.
    pub snmp: SnmpConfig,
    /// Ethernet MAC address (locally administered). [0;6] = not yet generated.
    /// Generated from hardware entropy on first boot and persisted.
    pub mac_addr: [u8; 6],
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
            ntp: NtpConfig::default(),
            syslog: SyslogConfig::default(),
            mqtt: MqttConfig::default(),
            snmp: SnmpConfig::default(),
            mac_addr: [0u8; 6],
        }
    }
}

// ---------------------------------------------------------------------------
// PointConfig
// ---------------------------------------------------------------------------

/// Per-point display and routing configuration, stored separately from `BridgeConfig`.
///
/// Each entry corresponds to one BACnet object on a discovered device. The
/// `scale` and `offset` fields allow raw BACnet values to be converted to
/// engineering units before publishing to MQTT or the UI. `state_text` maps
/// 1-based multi-state integers to human-readable labels (index 0 = state 1).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PointConfig {
    /// BACnet object type code (matches `ObjectType::code()`).
    pub object_type: u16,
    /// BACnet object instance number.
    pub object_instance: u32,
    /// Multiplicative scale applied to raw numeric values (default 1.0).
    pub scale: f32,
    /// Additive offset applied after scale: `display = raw * scale + offset` (default 0.0).
    pub offset: f32,
    /// BACnet engineering unit code (ASHRAE 135 enumeration). 95 = no units.
    pub engineering_unit: u16,
    /// Show this point on the admin dashboard.
    pub show_on_dashboard: bool,
    /// Forward this point's value changes to BACnet/IP subscribers.
    pub bridge_to_bacnet_ip: bool,
    /// Publish this point's value to the MQTT broker.
    pub bridge_to_mqtt: bool,
    /// Expose this point in the HTTP REST API.
    pub expose_in_api: bool,
    /// State text for multi-state objects. Index 0 maps to state 1 (BACnet is 1-based).
    /// Up to 16 states, each label up to 16 characters.
    pub state_text: Vec<String<16>, 16>,
}

impl Default for PointConfig {
    fn default() -> Self {
        Self {
            object_type: 0,
            object_instance: 0,
            scale: 1.0,
            offset: 0.0,
            engineering_unit: 95,
            show_on_dashboard: true,
            bridge_to_bacnet_ip: true,
            bridge_to_mqtt: true,
            expose_in_api: true,
            state_text: Vec::new(),
        }
    }
}

/// Valid MS/TP baud rates (per BACnet clause 9.3).
const VALID_BAUD_RATES: [u32; 4] = [9600, 19200, 38400, 76800];

/// Maximum BACnet device instance number (22-bit field, ASHRAE 135 clause 12.11).
pub const DEVICE_ID_MAX: u32 = 0x003F_FFFE;

impl BridgeConfig {
    /// Return true if the magic number, version, and all semantic fields are valid.
    ///
    /// Checks:
    /// - `magic` == `MAGIC`
    /// - `version` == `CONFIG_VERSION`
    /// - `bacnet.mstp_mac` <= 127
    /// - `bacnet.mstp_baud` is one of {9600, 19200, 38400, 76800}
    /// - `bacnet.device_id` <= 0x003F_FFFE (22-bit BACnet instance max)
    /// - `bacnet.max_master` >= 1 && <= 127
    /// - `hostname` is non-empty
    /// - `ntp.sync_interval_secs` >= 60
    /// - `syslog.port` > 0 when `syslog.enabled`
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
            // Warn (non-fatal): TLS is enabled but port is the plain-MQTT default.
            // We don't fail validation here — the user may have a broker that
            // accepts TLS on 1883 — but most don't.  The admin UI surfaces this.
            let _ = self.mqtt.tls_enabled && self.mqtt.port == 1883; // advisory only
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
        let mut buf = [0u8; 2048];
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
                password_hash: [0xABu8; 24],
                role: UserRole::Admin,
            })
            .expect("push failed");

        let mut buf = [0u8; 2048];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.users.len(), 1);
        assert_eq!(decoded.users[0].role, UserRole::Admin);
    }

    #[test]
    fn test_validate_mstp_mac_128_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_mac = 128;
        assert!(!cfg.validate(), "mstp_mac=128 should fail validation");
    }

    #[test]
    fn test_validate_bad_baud_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_baud = 115200; // not in the allowed set
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
        cfg.hostname = heapless::String::new(); // empty
        assert!(!cfg.validate(), "empty hostname should fail validation");
    }

    #[test]
    fn test_validate_all_good_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.bacnet.mstp_mac = 1;
        cfg.bacnet.mstp_baud = 9600;
        cfg.bacnet.device_id = 100;
        cfg.bacnet.max_master = 127;
        // hostname is "bacnet-bridge" by default — non-empty
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

    #[test]
    fn network_config_static_ip() {
        let mut cfg = BridgeConfig::default();
        cfg.network.dhcp = false;
        cfg.network.ip = [10, 0, 0, 50];
        cfg.network.subnet = [255, 255, 0, 0];

        let mut buf = [0u8; 2048];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(!decoded.network.dhcp);
        assert_eq!(decoded.network.ip, [10, 0, 0, 50]);
        assert_eq!(decoded.network.subnet, [255, 255, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // NtpConfig tests
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
        let mut buf = [0u8; 2048];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.ntp.enabled, cfg.ntp.enabled);
        assert_eq!(decoded.ntp.sync_interval_secs, cfg.ntp.sync_interval_secs);
        assert_eq!(decoded.ntp.servers[0].as_str(), "pool.ntp.org");
    }

    // -----------------------------------------------------------------------
    // SyslogConfig tests
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
        // port is 514 by default
        assert!(cfg.validate(), "syslog enabled with valid port should pass");
    }

    #[test]
    fn syslog_disabled_zero_port_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = false;
        cfg.syslog.port = 0;
        assert!(
            cfg.validate(),
            "syslog disabled with port=0 should pass (port only checked when enabled)"
        );
    }

    /// Regression: enabling syslog with an empty server hostname must fail
    /// validation even when the port is valid.
    #[test]
    fn syslog_enabled_empty_server_fails() {
        let mut cfg = BridgeConfig::default();
        cfg.syslog.enabled = true;
        cfg.syslog.server = String::new(); // empty — no server configured
        cfg.syslog.port = 514;
        assert!(
            !cfg.validate(),
            "syslog enabled with empty server must fail validation"
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

        let mut buf = [0u8; 2048];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert!(decoded.syslog.enabled);
        assert_eq!(decoded.syslog.server.as_str(), "syslog.local");
        assert_eq!(decoded.syslog.port, 514);
    }

    // -----------------------------------------------------------------------
    // MqttConfig tests
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
        // broker is empty by default
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
        // port is 1883 by default
        assert!(
            cfg.validate(),
            "mqtt enabled with broker and port should pass"
        );
    }

    #[test]
    fn mqtt_disabled_empty_broker_passes() {
        let mut cfg = BridgeConfig::default();
        cfg.mqtt.enabled = false;
        // broker is empty — fine when disabled
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

        let mut buf = [0u8; 4096];
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
    // SnmpConfig tests
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
        cfg.snmp.community = String::new(); // empty
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
        let mut buf = [0u8; 2048];
        let json = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (decoded, _): (BridgeConfig, _) =
            serde_json_core::from_slice(&buf[..json]).expect("deserialize failed");
        assert_eq!(decoded.snmp.enabled, cfg.snmp.enabled);
        assert_eq!(decoded.snmp.community.as_str(), "public");
    }

    // -----------------------------------------------------------------------
    // CONFIG_VERSION bump test
    // -----------------------------------------------------------------------

    #[test]
    fn config_version_is_2() {
        assert_eq!(CONFIG_VERSION, 2);
    }
}
