//! BACnet PDU types: object identifiers, property IDs, values, APDU/service enums.

use heapless::String;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ObjectType
// ---------------------------------------------------------------------------

/// BACnet object types (ASHRAE 135 clause 23.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum ObjectType {
    AnalogInput = 0,
    AnalogOutput = 1,
    AnalogValue = 2,
    BinaryInput = 3,
    BinaryOutput = 4,
    BinaryValue = 5,
    MultiStateInput = 13,
    MultiStateOutput = 14,
    MultiStateValue = 19,
    NotificationClass = 15,
    TrendLog = 20,
    Schedule = 17,
    Calendar = 6,
    Device = 8,
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

impl ObjectId {
    pub fn new(object_type: ObjectType, instance: u32) -> Self {
        Self { object_type, instance }
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
        ObjectType::from_code(type_code).map(|t| Self { object_type: t, instance })
    }
}

// ---------------------------------------------------------------------------
// PropertyId
// ---------------------------------------------------------------------------

/// BACnet property identifiers (ASHRAE 135 clause 23.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyId {
    /// Present-Value property (85).
    PresentValue,
    /// Object-Name property (77).
    ObjectName,
    /// Description property (28).
    Description,
    /// Units property (117).
    Units,
    /// Status-Flags property (111).
    StatusFlags,
    /// Object-Type property (79).
    ObjectType,
    /// Object-Identifier property (75).
    ObjectIdentifier,
    /// Out-Of-Service property (81).
    OutOfService,
    /// Device-Type property (31).
    DeviceType,
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

/// BACnet APDU type codes (bits 7–4 of the first APDU byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ApduType {
    ConfirmedRequest = 0x00,
    UnconfirmedRequest = 0x10,
    SimpleAck = 0x20,
    ComplexAck = 0x30,
    Error = 0x50,
    Reject = 0x60,
    Abort = 0x70,
}

impl ApduType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0xF0 {
            0x00 => Some(Self::ConfirmedRequest),
            0x10 => Some(Self::UnconfirmedRequest),
            0x20 => Some(Self::SimpleAck),
            0x30 => Some(Self::ComplexAck),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceChoice {
    // Confirmed services
    ReadProperty,
    WriteProperty,
    ReadPropertyMultiple,
    SubscribeCOV,
    // Unconfirmed services
    WhoIs,
    IAm,
    WhoHas,
    IHave,
    TimeSynchronization,
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
            Self::IHave => Some(7),
            Self::WhoIs => Some(8),
            Self::TimeSynchronization => Some(6),
            Self::WhoHas => Some(7),
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
            6 => Some(Self::TimeSynchronization),
            7 => Some(Self::IHave),
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
    fn apdu_type_from_byte() {
        assert_eq!(ApduType::from_byte(0x00), Some(ApduType::ConfirmedRequest));
        assert_eq!(ApduType::from_byte(0x0F), Some(ApduType::ConfirmedRequest));
        assert_eq!(ApduType::from_byte(0x10), Some(ApduType::UnconfirmedRequest));
        assert_eq!(ApduType::from_byte(0x20), Some(ApduType::SimpleAck));
        assert_eq!(ApduType::from_byte(0x30), Some(ApduType::ComplexAck));
        assert_eq!(ApduType::from_byte(0x50), Some(ApduType::Error));
        assert_eq!(ApduType::from_byte(0x60), Some(ApduType::Reject));
        assert_eq!(ApduType::from_byte(0x70), Some(ApduType::Abort));
        assert_eq!(ApduType::from_byte(0x40), None);
    }

    #[test]
    fn service_choice_confirmed_codes() {
        assert_eq!(ServiceChoice::ReadProperty.confirmed_code(), Some(12));
        assert_eq!(ServiceChoice::WriteProperty.confirmed_code(), Some(15));
        assert_eq!(ServiceChoice::ReadPropertyMultiple.confirmed_code(), Some(14));
        assert_eq!(ServiceChoice::SubscribeCOV.confirmed_code(), Some(5));
        assert_eq!(ServiceChoice::WhoIs.confirmed_code(), None);
    }

    #[test]
    fn service_choice_unconfirmed_round_trip() {
        let choices = [
            (ServiceChoice::IAm, 0u8),
            (ServiceChoice::TimeSynchronization, 6u8),
            (ServiceChoice::WhoIs, 8u8),
        ];
        for (svc, code) in choices {
            assert_eq!(svc.unconfirmed_code(), Some(code));
            let recovered = ServiceChoice::from_unconfirmed_code(code).unwrap();
            assert_eq!(recovered, svc);
        }
    }
}
