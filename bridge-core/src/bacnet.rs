//! BACnet protocol types.
//!
//! Numeric codes are per ASHRAE 135-2020 and match the bacnet-stack
//! enumerations in `lib/bacnet-stack/src/bacnet/bacenum.h`.

use heapless::String;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ObjectType
// ---------------------------------------------------------------------------

/// BACnet object types (ASHRAE 135 clause 23.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum ObjectType {
    /// ASHRAE object type 0 — OBJECT_ANALOG_INPUT
    AnalogInput = 0,
    /// ASHRAE object type 1 — OBJECT_ANALOG_OUTPUT
    AnalogOutput = 1,
    /// ASHRAE object type 2 — OBJECT_ANALOG_VALUE
    AnalogValue = 2,
    /// ASHRAE object type 3 — OBJECT_BINARY_INPUT
    BinaryInput = 3,
    /// ASHRAE object type 4 — OBJECT_BINARY_OUTPUT
    BinaryOutput = 4,
    /// ASHRAE object type 5 — OBJECT_BINARY_VALUE
    BinaryValue = 5,
    /// ASHRAE object type 6 — OBJECT_CALENDAR
    Calendar = 6,
    /// ASHRAE object type 8 — OBJECT_DEVICE
    Device = 8,
    /// ASHRAE object type 13 — OBJECT_MULTI_STATE_INPUT
    MultiStateInput = 13,
    /// ASHRAE object type 14 — OBJECT_MULTI_STATE_OUTPUT
    MultiStateOutput = 14,
    /// ASHRAE object type 15 — OBJECT_NOTIFICATION_CLASS
    NotificationClass = 15,
    /// ASHRAE object type 17 — OBJECT_SCHEDULE
    Schedule = 17,
    /// ASHRAE object type 19 — OBJECT_MULTI_STATE_VALUE
    MultiStateValue = 19,
    /// ASHRAE object type 20 — OBJECT_TRENDLOG
    TrendLog = 20,
}

impl ObjectType {
    /// Convert from the standard BACnet numeric code.
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            0 => Some(Self::AnalogInput),
            1 => Some(Self::AnalogOutput),
            2 => Some(Self::AnalogValue),
            3 => Some(Self::BinaryInput),
            4 => Some(Self::BinaryOutput),
            5 => Some(Self::BinaryValue),
            6 => Some(Self::Calendar),
            8 => Some(Self::Device),
            13 => Some(Self::MultiStateInput),
            14 => Some(Self::MultiStateOutput),
            15 => Some(Self::NotificationClass),
            17 => Some(Self::Schedule),
            19 => Some(Self::MultiStateValue),
            20 => Some(Self::TrendLog),
            _ => None,
        }
    }

    /// Return the standard BACnet numeric code for this type.
    pub fn code(self) -> u16 {
        self as u16
    }
}

impl From<ObjectType> for u16 {
    fn from(v: ObjectType) -> u16 {
        v.code()
    }
}

// ---------------------------------------------------------------------------
// ObjectId
// ---------------------------------------------------------------------------

/// A BACnet object identifier: type + instance number (0–4194302).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectId {
    pub object_type: ObjectType,
    /// Object instance number (22 bits; max 4194302).
    pub instance: u32,
}

/// Maximum valid BACnet object instance number (22-bit field, ASHRAE 135 §12.11).
/// Values 0x003F_FFFF and 0x003F_FFFE are reserved; usable range is 0..=4194302.
pub const INSTANCE_MAX: u32 = 0x003F_FFFE;

impl ObjectId {
    /// Create an `ObjectId` from a type and instance number.
    ///
    /// The `instance` value must be in the range `0..=0x003F_FFFE` (22 bits,
    /// BACnet instance maximum). Values outside this range are undefined
    /// behaviour per ASHRAE 135; a `debug_assert!` will catch this in debug
    /// builds. Use [`ObjectId::try_new`] for a checked, safe alternative.
    pub fn new(object_type: ObjectType, instance: u32) -> Self {
        debug_assert!(
            instance <= INSTANCE_MAX,
            "ObjectId instance {instance} exceeds INSTANCE_MAX (0x003F_FFFE = {INSTANCE_MAX})"
        );
        Self {
            object_type,
            instance,
        }
    }

    /// Create an `ObjectId`, returning `None` if `instance` exceeds
    /// `INSTANCE_MAX` (0x003F_FFFE).
    ///
    /// Prefer this over [`ObjectId::new`] when the instance number comes from
    /// an untrusted source (e.g. decoded from a BACnet packet).
    pub fn try_new(object_type: ObjectType, instance: u32) -> Option<Self> {
        if instance > INSTANCE_MAX {
            return None;
        }
        Some(Self {
            object_type,
            instance,
        })
    }

    /// Encode as a 32-bit BACnet object identifier value:
    /// bits 31–22 = type, bits 21–0 = instance.
    pub fn to_raw(self) -> u32 {
        ((self.object_type.code() as u32) << 22) | (self.instance & 0x003F_FFFF)
    }

    /// Decode from a 32-bit BACnet object identifier value.
    pub fn from_raw(raw: u32) -> Option<Self> {
        let type_code = ((raw >> 22) & 0x3FF) as u16;
        let instance = raw & 0x003F_FFFF;
        ObjectType::from_code(type_code).map(|t| Self {
            object_type: t,
            instance,
        })
    }
}

// ---------------------------------------------------------------------------
// PropertyId
// ---------------------------------------------------------------------------

/// BACnet property identifiers (ASHRAE 135 clause 23.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyId {
    /// Description property — ASHRAE property 28 (PROP_DESCRIPTION)
    Description,
    /// Device-Type property — ASHRAE property 31 (PROP_DEVICE_TYPE)
    DeviceType,
    /// Object-Identifier property — ASHRAE property 75 (PROP_OBJECT_IDENTIFIER)
    ObjectIdentifier,
    /// Object-Name property — ASHRAE property 77 (PROP_OBJECT_NAME)
    ObjectName,
    /// Object-Type property — ASHRAE property 79 (PROP_OBJECT_TYPE)
    ObjectType,
    /// Out-Of-Service property — ASHRAE property 81 (PROP_OUT_OF_SERVICE)
    OutOfService,
    /// Present-Value property — ASHRAE property 85 (PROP_PRESENT_VALUE)
    PresentValue,
    /// Status-Flags property — ASHRAE property 111 (PROP_STATUS_FLAGS)
    StatusFlags,
    /// Units property — ASHRAE property 117 (PROP_UNITS)
    Units,
    /// An unknown or vendor-specific property identified by its raw code.
    Raw(u32),
}

impl PropertyId {
    pub fn from_code(code: u32) -> Self {
        match code {
            28 => Self::Description,
            31 => Self::DeviceType,
            75 => Self::ObjectIdentifier,
            77 => Self::ObjectName,
            79 => Self::ObjectType,
            81 => Self::OutOfService,
            85 => Self::PresentValue,
            111 => Self::StatusFlags,
            117 => Self::Units,
            other => Self::Raw(other),
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Self::Description => 28,
            Self::DeviceType => 31,
            Self::ObjectIdentifier => 75,
            Self::ObjectName => 77,
            Self::ObjectType => 79,
            Self::OutOfService => 81,
            Self::PresentValue => 85,
            Self::StatusFlags => 111,
            Self::Units => 117,
            Self::Raw(v) => v,
        }
    }
}

impl From<PropertyId> for u32 {
    fn from(p: PropertyId) -> u32 {
        p.code()
    }
}

impl From<u32> for PropertyId {
    fn from(v: u32) -> Self {
        Self::from_code(v)
    }
}

// ---------------------------------------------------------------------------
// BacnetValue
// ---------------------------------------------------------------------------

/// A typed BACnet property value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BacnetValue {
    Null,
    Boolean(bool),
    UnsignedInt(u32),
    SignedInt(i32),
    Real(f32),
    /// UTF-8 character string, max 64 bytes.
    CharString(String<64>),
    Enumerated(u32),
    ObjectIdentifier(ObjectId),
}

// ---------------------------------------------------------------------------
// ApduType
// ---------------------------------------------------------------------------

/// BACnet APDU type codes (BACNET_PDU_TYPE in bacenum.h).
///
/// These are the high-nibble values of the first APDU byte (bits 7–4).
/// `from_byte` masks the input with `0xF0` to extract the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ApduType {
    /// PDU_TYPE_CONFIRMED_SERVICE_REQUEST = 0x00
    ConfirmedRequest = 0x00,
    /// PDU_TYPE_UNCONFIRMED_SERVICE_REQUEST = 0x10
    UnconfirmedRequest = 0x10,
    /// PDU_TYPE_SIMPLE_ACK = 0x20
    SimpleAck = 0x20,
    /// PDU_TYPE_COMPLEX_ACK = 0x30
    ComplexAck = 0x30,
    /// PDU_TYPE_SEGMENT_ACK = 0x40
    SegmentAck = 0x40,
    /// PDU_TYPE_ERROR = 0x50
    Error = 0x50,
    /// PDU_TYPE_REJECT = 0x60
    Reject = 0x60,
    /// PDU_TYPE_ABORT = 0x70
    Abort = 0x70,
}

impl ApduType {
    /// Parse an APDU type from the first byte of an APDU.
    ///
    /// Masks the high nibble (bits 7–4) and matches against the known PDU types.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0xF0 {
            0x00 => Some(Self::ConfirmedRequest),
            0x10 => Some(Self::UnconfirmedRequest),
            0x20 => Some(Self::SimpleAck),
            0x30 => Some(Self::ComplexAck),
            0x40 => Some(Self::SegmentAck),
            0x50 => Some(Self::Error),
            0x60 => Some(Self::Reject),
            0x70 => Some(Self::Abort),
            _ => None,
        }
    }
}

impl From<ApduType> for u8 {
    fn from(a: ApduType) -> u8 {
        a as u8
    }
}

// ---------------------------------------------------------------------------
// ServiceChoice
// ---------------------------------------------------------------------------

/// BACnet confirmed and unconfirmed service choice codes.
///
/// Confirmed codes match `BACNET_CONFIRMED_SERVICE` in bacenum.h.
/// Unconfirmed codes match `BACNET_UNCONFIRMED_SERVICE` in bacenum.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceChoice {
    // Confirmed services
    /// SERVICE_CONFIRMED_SUBSCRIBE_COV = 5
    SubscribeCOV,
    /// SERVICE_CONFIRMED_READ_PROPERTY = 12
    ReadProperty,
    /// SERVICE_CONFIRMED_READ_PROP_MULTIPLE = 14
    ReadPropertyMultiple,
    /// SERVICE_CONFIRMED_WRITE_PROPERTY = 15
    WriteProperty,
    // Unconfirmed services
    /// SERVICE_UNCONFIRMED_I_AM = 0
    IAm,
    /// SERVICE_UNCONFIRMED_I_HAVE = 1
    IHave,
    /// SERVICE_UNCONFIRMED_TIME_SYNCHRONIZATION = 6
    TimeSynchronization,
    /// SERVICE_UNCONFIRMED_WHO_HAS = 7
    WhoHas,
    /// SERVICE_UNCONFIRMED_WHO_IS = 8
    WhoIs,
}

impl ServiceChoice {
    /// Return the BACnet service choice byte for confirmed services.
    pub fn confirmed_code(self) -> Option<u8> {
        match self {
            Self::SubscribeCOV => Some(5),
            Self::ReadProperty => Some(12),
            Self::ReadPropertyMultiple => Some(14),
            Self::WriteProperty => Some(15),
            _ => None,
        }
    }

    /// Return the BACnet service choice byte for unconfirmed services.
    pub fn unconfirmed_code(self) -> Option<u8> {
        match self {
            Self::IAm => Some(0),
            Self::IHave => Some(1),
            Self::TimeSynchronization => Some(6),
            Self::WhoHas => Some(7),
            Self::WhoIs => Some(8),
            _ => None,
        }
    }

    pub fn from_confirmed_code(code: u8) -> Option<Self> {
        match code {
            5 => Some(Self::SubscribeCOV),
            12 => Some(Self::ReadProperty),
            14 => Some(Self::ReadPropertyMultiple),
            15 => Some(Self::WriteProperty),
            _ => None,
        }
    }

    pub fn from_unconfirmed_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::IAm),
            1 => Some(Self::IHave),
            6 => Some(Self::TimeSynchronization),
            7 => Some(Self::WhoHas),
            8 => Some(Self::WhoIs),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_type_round_trip() {
        let types = [
            ObjectType::AnalogInput,
            ObjectType::AnalogOutput,
            ObjectType::AnalogValue,
            ObjectType::BinaryInput,
            ObjectType::BinaryOutput,
            ObjectType::BinaryValue,
            ObjectType::MultiStateInput,
            ObjectType::MultiStateOutput,
            ObjectType::MultiStateValue,
            ObjectType::NotificationClass,
            ObjectType::TrendLog,
            ObjectType::Schedule,
            ObjectType::Calendar,
            ObjectType::Device,
        ];
        for t in types {
            let code = t.code();
            let recovered = ObjectType::from_code(code).expect("round-trip failed");
            assert_eq!(t, recovered, "ObjectType round-trip failed for {:?}", t);
        }
    }

    #[test]
    fn object_type_known_codes() {
        assert_eq!(ObjectType::AnalogInput.code(), 0);
        assert_eq!(ObjectType::AnalogOutput.code(), 1);
        assert_eq!(ObjectType::Device.code(), 8);
        assert_eq!(ObjectType::MultiStateValue.code(), 19);
        assert_eq!(ObjectType::TrendLog.code(), 20);
    }

    #[test]
    fn object_type_unknown_code() {
        assert!(ObjectType::from_code(255).is_none());
        assert!(ObjectType::from_code(9).is_none());
    }

    #[test]
    fn property_id_round_trip() {
        let props = [
            PropertyId::PresentValue,
            PropertyId::ObjectName,
            PropertyId::Description,
            PropertyId::Units,
            PropertyId::StatusFlags,
            PropertyId::ObjectType,
            PropertyId::ObjectIdentifier,
            PropertyId::OutOfService,
            PropertyId::DeviceType,
        ];
        for p in props {
            let code = p.code();
            let recovered = PropertyId::from_code(code);
            assert_eq!(p, recovered, "PropertyId round-trip failed for {:?}", p);
        }
    }

    #[test]
    fn property_id_raw_variant() {
        let p = PropertyId::from_code(9999);
        assert_eq!(p, PropertyId::Raw(9999));
        assert_eq!(p.code(), 9999);
    }

    #[test]
    fn object_id_raw_encoding() {
        let oid = ObjectId::new(ObjectType::AnalogInput, 42);
        let raw = oid.to_raw();
        // AnalogInput = 0, so raw = 42
        assert_eq!(raw, 42);
        let recovered = ObjectId::from_raw(raw).unwrap();
        assert_eq!(recovered, oid);
    }

    #[test]
    fn object_id_device_encoding() {
        // Device = type 8, instance 389999
        let oid = ObjectId::new(ObjectType::Device, 389999);
        let raw = oid.to_raw();
        let recovered = ObjectId::from_raw(raw).unwrap();
        assert_eq!(recovered.object_type, ObjectType::Device);
        assert_eq!(recovered.instance, 389999);
    }

    #[test]
    fn test_object_id_max_valid_instance() {
        // INSTANCE_MAX itself must be accepted by both new() and try_new().
        let oid = ObjectId::new(ObjectType::AnalogInput, INSTANCE_MAX);
        assert_eq!(oid.instance, INSTANCE_MAX);
        let oid2 = ObjectId::try_new(ObjectType::AnalogInput, INSTANCE_MAX);
        assert!(oid2.is_some());
        assert_eq!(oid2.unwrap().instance, INSTANCE_MAX);
    }

    #[test]
    fn test_object_id_try_new_overflow_returns_none() {
        // Any instance > INSTANCE_MAX must return None from try_new().
        assert!(
            ObjectId::try_new(ObjectType::AnalogInput, INSTANCE_MAX + 1).is_none(),
            "try_new with instance=INSTANCE_MAX+1 should return None"
        );
        assert!(
            ObjectId::try_new(ObjectType::Device, u32::MAX).is_none(),
            "try_new with instance=u32::MAX should return None"
        );
    }

    #[test]
    fn test_object_id_try_new_zero_is_valid() {
        let oid = ObjectId::try_new(ObjectType::BinaryInput, 0);
        assert!(oid.is_some());
        assert_eq!(oid.unwrap().instance, 0);
    }

    #[test]
    fn apdu_type_from_byte() {
        assert_eq!(ApduType::from_byte(0x00), Some(ApduType::ConfirmedRequest));
        assert_eq!(ApduType::from_byte(0x0F), Some(ApduType::ConfirmedRequest));
        assert_eq!(
            ApduType::from_byte(0x10),
            Some(ApduType::UnconfirmedRequest)
        );
        assert_eq!(ApduType::from_byte(0x20), Some(ApduType::SimpleAck));
        assert_eq!(ApduType::from_byte(0x30), Some(ApduType::ComplexAck));
        assert_eq!(ApduType::from_byte(0x40), Some(ApduType::SegmentAck));
        assert_eq!(ApduType::from_byte(0x50), Some(ApduType::Error));
        assert_eq!(ApduType::from_byte(0x60), Some(ApduType::Reject));
        assert_eq!(ApduType::from_byte(0x70), Some(ApduType::Abort));
    }

    #[test]
    fn service_choice_confirmed_codes() {
        assert_eq!(ServiceChoice::ReadProperty.confirmed_code(), Some(12));
        assert_eq!(ServiceChoice::WriteProperty.confirmed_code(), Some(15));
        assert_eq!(
            ServiceChoice::ReadPropertyMultiple.confirmed_code(),
            Some(14)
        );
        assert_eq!(ServiceChoice::SubscribeCOV.confirmed_code(), Some(5));
        assert_eq!(ServiceChoice::WhoIs.confirmed_code(), None);
    }

    #[test]
    fn service_choice_unconfirmed_round_trip() {
        let choices = [
            (ServiceChoice::IAm, 0u8),
            (ServiceChoice::IHave, 1u8),
            (ServiceChoice::TimeSynchronization, 6u8),
            (ServiceChoice::WhoHas, 7u8),
            (ServiceChoice::WhoIs, 8u8),
        ];
        for (svc, code) in choices {
            assert_eq!(svc.unconfirmed_code(), Some(code));
            let recovered = ServiceChoice::from_unconfirmed_code(code).unwrap();
            assert_eq!(recovered, svc);
        }
    }

    #[test]
    fn ihave_is_not_whohas() {
        // Regression test: IHave must be code 1, WhoHas must be code 7.
        // Previously IHave was incorrectly assigned code 7.
        assert_ne!(
            ServiceChoice::IHave.unconfirmed_code(),
            ServiceChoice::WhoHas.unconfirmed_code()
        );
        assert_eq!(ServiceChoice::IHave.unconfirmed_code(), Some(1));
        assert_eq!(ServiceChoice::WhoHas.unconfirmed_code(), Some(7));
        assert_eq!(
            ServiceChoice::from_unconfirmed_code(1),
            Some(ServiceChoice::IHave)
        );
        assert_eq!(
            ServiceChoice::from_unconfirmed_code(7),
            Some(ServiceChoice::WhoHas)
        );
    }
}
