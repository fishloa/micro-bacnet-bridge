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
// EngineeringUnits
// ---------------------------------------------------------------------------

/// BACnet engineering units (ASHRAE 135 enumeration).
///
/// Converts to/from numeric codes and maps to Home Assistant unit strings.
/// When a BACnet unit is not directly supported by HA, this enum provides
/// conversion to the closest HA unit with a scaling factor.
///
/// Numeric codes match `BACNET_ENGINEERING_UNITS` in bacnet-stack/bacenum.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum EngineeringUnits {
    // Dimensionless / Percent
    /// Percentage (0–100 or 0–1 depending on context)
    Percent = 95,
    /// Parts per million (ppm)
    Ppm = 97,
    /// RPM (revolutions per minute)
    Rpm = 123,

    // Temperature
    /// Degrees Celsius
    DegreesCelsius = 0,
    /// Degrees Fahrenheit
    DegreesFahrenheit = 1,
    /// Kelvin
    Kelvin = 2,

    // Pressure
    /// Pascals
    Pascal = 3,
    /// Kilopascals
    Kilopascal = 4,
    /// Bar
    Bar = 5,
    /// Pounds per square inch (PSI)
    Psi = 6,
    /// Millimeters of water column (mmH₂O)
    MillimetersWater = 7,
    /// Centimeters of water column (cmH₂O)
    CentimetersWater = 8,
    /// Inches of mercury (inHg)
    InchesOfMercury = 9,
    /// Centimeters of mercury (cmHg)
    CentimetersOfMercury = 10,
    /// Millimeters of mercury (mmHg)
    MillimetersOfMercury = 11,

    // Energy / Power
    /// Joules
    Joule = 12,
    /// Kilojoules
    Kilojoule = 13,
    /// Kilowatt-hours
    KilowattHour = 14,
    /// Megawatt-hours
    MegawattHour = 15,
    /// Watt-hours
    WattHour = 16,
    /// BTU
    Btu = 17,
    /// BTU per hour
    BtuPerHour = 18,
    /// Watts
    Watt = 19,
    /// Kilowatts
    Kilowatt = 20,
    /// Megawatts
    Megawatt = 21,
    /// Therms
    Therm = 22,
    /// Ton-hours
    TonHour = 23,

    // Flow
    /// Cubic meters per second
    CubicMeterPerSecond = 24,
    /// Cubic feet per second
    CubicFootPerSecond = 25,
    /// Cubic feet per minute
    CubicFootPerMinute = 26,
    /// Cubic meters per hour
    CubicMeterPerHour = 27,
    /// Gallons per minute (US)
    GallonPerMinute = 28,
    /// Liters per second
    LiterPerSecond = 29,
    /// Liters per minute
    LiterPerMinute = 30,

    // Distance
    /// Millimeters
    Millimeter = 32,
    /// Centimeters
    Centimeter = 33,
    /// Meters
    Meter = 34,
    /// Inches
    Inch = 35,
    /// Feet
    Foot = 36,
    /// Yards
    Yard = 37,
    /// Kilometers
    Kilometer = 38,
    /// Miles
    Mile = 39,

    // Mass
    /// Grams
    Gram = 41,
    /// Kilograms
    Kilogram = 42,
    /// Metric tons (tonnes)
    MetricTon = 43,
    /// Pounds
    Pound = 44,
    /// Ounces
    Ounce = 45,
    /// Ton (short US ton)
    Ton = 46,

    // Volume
    /// Cubic meters
    CubicMeter = 48,
    /// Cubic feet
    CubicFoot = 49,
    /// Gallons (US)
    Gallon = 50,
    /// Liters
    Liter = 51,
    /// Milliliters
    Milliliter = 52,

    // Electrical
    /// Amperes
    Ampere = 58,
    /// Milliamperes
    Milliampere = 59,
    /// Volts
    Volt = 60,
    /// Millivolts
    Millivolt = 61,
    /// Hertz
    Hertz = 65,
    /// Gigahertz
    Gigahertz = 66,

    // Light
    /// Lux
    Lux = 71,
    /// Lumens
    Lumen = 72,

    // Sound
    /// Decibels
    Decibel = 73,
    /// Decibels milliwatts
    DecibelMilliwatt = 74,

    // Velocity / Speed
    /// Meters per second
    MeterPerSecond = 62,
    /// Kilometers per hour
    KilometerPerHour = 63,
    /// Miles per hour
    MilePerHour = 64,
    /// Millimeters per day (rainfall)
    MillimeterPerDay = 67,
    /// Inches per day (rainfall)
    InchPerDay = 68,

    // Time
    /// Seconds
    Second = 69,
    /// Minutes
    Minute = 70,
    /// Hours
    Hour = 75,
    /// Days
    Day = 76,
    /// Weeks
    Week = 77,
    /// Months
    Month = 78,
    /// Years
    Year = 79,

    // Power / Energy (continued)
    /// Horsepower
    Horsepower = 80,

    // Other
    /// No units / dimensionless (placeholder, rarely used)
    NoUnits = 196,
}

impl EngineeringUnits {
    /// Convert from BACnet numeric code. Returns `None` if the code is unknown.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::DegreesCelsius),
            1 => Some(Self::DegreesFahrenheit),
            2 => Some(Self::Kelvin),
            3 => Some(Self::Pascal),
            4 => Some(Self::Kilopascal),
            5 => Some(Self::Bar),
            6 => Some(Self::Psi),
            7 => Some(Self::MillimetersWater),
            8 => Some(Self::CentimetersWater),
            9 => Some(Self::InchesOfMercury),
            10 => Some(Self::CentimetersOfMercury),
            11 => Some(Self::MillimetersOfMercury),
            12 => Some(Self::Joule),
            13 => Some(Self::Kilojoule),
            14 => Some(Self::KilowattHour),
            15 => Some(Self::MegawattHour),
            16 => Some(Self::WattHour),
            17 => Some(Self::Btu),
            18 => Some(Self::BtuPerHour),
            19 => Some(Self::Watt),
            20 => Some(Self::Kilowatt),
            21 => Some(Self::Megawatt),
            22 => Some(Self::Therm),
            23 => Some(Self::TonHour),
            24 => Some(Self::CubicMeterPerSecond),
            25 => Some(Self::CubicFootPerSecond),
            26 => Some(Self::CubicFootPerMinute),
            27 => Some(Self::CubicMeterPerHour),
            28 => Some(Self::GallonPerMinute),
            29 => Some(Self::LiterPerSecond),
            30 => Some(Self::LiterPerMinute),
            32 => Some(Self::Millimeter),
            33 => Some(Self::Centimeter),
            34 => Some(Self::Meter),
            35 => Some(Self::Inch),
            36 => Some(Self::Foot),
            37 => Some(Self::Yard),
            38 => Some(Self::Kilometer),
            39 => Some(Self::Mile),
            41 => Some(Self::Gram),
            42 => Some(Self::Kilogram),
            43 => Some(Self::MetricTon),
            44 => Some(Self::Pound),
            45 => Some(Self::Ounce),
            46 => Some(Self::Ton),
            48 => Some(Self::CubicMeter),
            49 => Some(Self::CubicFoot),
            50 => Some(Self::Gallon),
            51 => Some(Self::Liter),
            52 => Some(Self::Milliliter),
            58 => Some(Self::Ampere),
            59 => Some(Self::Milliampere),
            60 => Some(Self::Volt),
            61 => Some(Self::Millivolt),
            62 => Some(Self::MeterPerSecond),
            63 => Some(Self::KilometerPerHour),
            64 => Some(Self::MilePerHour),
            65 => Some(Self::Hertz),
            66 => Some(Self::Gigahertz),
            67 => Some(Self::MillimeterPerDay),
            68 => Some(Self::InchPerDay),
            69 => Some(Self::Second),
            70 => Some(Self::Minute),
            71 => Some(Self::Lux),
            72 => Some(Self::Lumen),
            73 => Some(Self::Decibel),
            74 => Some(Self::DecibelMilliwatt),
            75 => Some(Self::Hour),
            76 => Some(Self::Day),
            77 => Some(Self::Week),
            78 => Some(Self::Month),
            79 => Some(Self::Year),
            80 => Some(Self::Horsepower),
            95 => Some(Self::Percent),
            97 => Some(Self::Ppm),
            123 => Some(Self::Rpm),
            196 => Some(Self::NoUnits),
            _ => None,
        }
    }

    /// Return the BACnet numeric code for this unit.
    pub fn code(self) -> u32 {
        self as u32
    }

    /// Return the Home Assistant unit string for this unit.
    ///
    /// If the BACnet unit is not directly supported by HA, returns a string
    /// that represents the closest HA-supported unit. Call [`convert_for_ha`]
    /// to get the scaled value.
    ///
    /// For unknown/unmapped units, returns an empty string.
    pub fn ha_unit_str(self) -> &'static str {
        match self {
            // Temperature
            Self::DegreesCelsius => "°C",
            Self::DegreesFahrenheit => "°F",
            Self::Kelvin => "K",

            // Pressure (pass-through where HA has them)
            Self::Pascal => "Pa",
            Self::Kilopascal => "kPa",
            Self::Bar => "bar",
            Self::Psi => "psi",
            Self::InchesOfMercury => "inHg",
            Self::MillimetersOfMercury => "mmHg",

            // Pressure conversions (BACnet → HA Pa)
            // mmH₂O, cmH₂O → Pa (HA doesn't have mmH₂O or cmH₂O)
            Self::MillimetersWater => "Pa", // multiply by 9.80665
            Self::CentimetersWater => "Pa", // multiply by 98.0665
            Self::CentimetersOfMercury => "mmHg", // multiply by 10 (close to HA's mmHg)

            // Energy / Power
            Self::Joule => "Wh",     // divide by 3600
            Self::Kilojoule => "Wh", // divide by 3.6
            Self::WattHour => "Wh",
            Self::KilowattHour => "kWh",
            Self::MegawattHour => "MWh",
            Self::Btu => "Wh",       // multiply by 0.293071
            Self::BtuPerHour => "W", // multiply by 0.293071
            Self::Watt => "W",
            Self::Kilowatt => "kW",
            Self::Megawatt => "MW",
            Self::Therm => "Wh",     // multiply by 293071
            Self::TonHour => "Wh",   // multiply by 3517000 (depends on ton type)
            Self::Horsepower => "W", // multiply by 745.7

            // Flow
            Self::CubicMeterPerSecond => "m³/s",
            Self::CubicFootPerSecond => "m³/s", // multiply by 0.0283168
            Self::CubicFootPerMinute => "L/min", // multiply by 0.471947
            Self::CubicMeterPerHour => "m³/h",
            Self::GallonPerMinute => "L/min", // multiply by 3.785
            Self::LiterPerSecond => "L/min",  // multiply by 60
            Self::LiterPerMinute => "L/min",

            // Distance
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Meter => "m",
            Self::Inch => "in",
            Self::Foot => "ft",
            Self::Yard => "yd",
            Self::Kilometer => "km",
            Self::Mile => "mi",

            // Mass
            Self::Gram => "g",
            Self::Kilogram => "kg",
            Self::MetricTon => "kg", // multiply by 1000
            Self::Pound => "lb",
            Self::Ounce => "oz",
            Self::Ton => "lb", // multiply by 2000 (short ton)

            // Volume
            Self::CubicMeter => "m³",
            Self::CubicFoot => "ft³",
            Self::Gallon => "gal",
            Self::Liter => "L",
            Self::Milliliter => "mL",

            // Electrical
            Self::Ampere => "A",
            Self::Milliampere => "mA",
            Self::Volt => "V",
            Self::Millivolt => "mV",
            Self::Hertz => "Hz",
            Self::Gigahertz => "GHz",

            // Light
            Self::Lux => "lx",
            Self::Lumen => "lm",

            // Sound
            Self::Decibel => "dB",
            Self::DecibelMilliwatt => "dBm",

            // Velocity / Speed
            Self::MeterPerSecond => "m/s",
            Self::KilometerPerHour => "km/h",
            Self::MilePerHour => "mph",
            Self::MillimeterPerDay => "mm/d",
            Self::InchPerDay => "in/d",

            // Time
            Self::Second => "s",
            Self::Minute => "min",
            Self::Hour => "h",
            Self::Day => "d",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",

            // Dimensionless
            Self::Percent => "%",
            Self::Ppm => "ppm",
            Self::Rpm => "rpm",
            Self::NoUnits => "",
        }
    }

    /// Convert a value from BACnet units to Home Assistant units.
    ///
    /// Returns a tuple: `(converted_value, ha_unit_string)`.
    ///
    /// For units that HA supports natively, returns the value unchanged.
    /// For units that require conversion, applies the appropriate scaling factor.
    /// For unknown codes, returns the value unchanged with an empty unit string.
    ///
    /// # Examples
    /// ```
    /// use bridge_core::bacnet::EngineeringUnits;
    ///
    /// // mmH₂O → Pa
    /// let (val, unit) = EngineeringUnits::MillimetersWater.convert_for_ha(100.0_f32);
    /// assert!((val - 980.665_f32).abs() < 0.01_f32);
    /// assert_eq!(unit, "Pa");
    ///
    /// // °C → °C (pass-through)
    /// let (val, unit) = EngineeringUnits::DegreesCelsius.convert_for_ha(25.0_f32);
    /// assert_eq!(val, 25.0_f32);
    /// assert_eq!(unit, "°C");
    /// ```
    pub fn convert_for_ha(self, value: f32) -> (f32, &'static str) {
        let unit_str = self.ha_unit_str();
        let converted = match self {
            // Pass-through (no conversion needed)
            Self::DegreesCelsius
            | Self::DegreesFahrenheit
            | Self::Kelvin
            | Self::Pascal
            | Self::Kilopascal
            | Self::Bar
            | Self::Psi
            | Self::InchesOfMercury
            | Self::MillimetersOfMercury
            | Self::Watt
            | Self::Kilowatt
            | Self::Megawatt
            | Self::WattHour
            | Self::KilowattHour
            | Self::MegawattHour
            | Self::Volt
            | Self::Millivolt
            | Self::Ampere
            | Self::Milliampere
            | Self::Hertz
            | Self::Gigahertz
            | Self::Percent
            | Self::Ppm
            | Self::Rpm
            | Self::Millimeter
            | Self::Centimeter
            | Self::Meter
            | Self::Inch
            | Self::Foot
            | Self::Yard
            | Self::Kilometer
            | Self::Mile
            | Self::Gram
            | Self::Kilogram
            | Self::Pound
            | Self::Ounce
            | Self::CubicMeter
            | Self::CubicMeterPerSecond
            | Self::CubicMeterPerHour
            | Self::CubicFoot
            | Self::Gallon
            | Self::Liter
            | Self::LiterPerMinute
            | Self::Milliliter
            | Self::Lux
            | Self::Lumen
            | Self::Decibel
            | Self::DecibelMilliwatt
            | Self::MeterPerSecond
            | Self::KilometerPerHour
            | Self::MilePerHour
            | Self::MillimeterPerDay
            | Self::InchPerDay
            | Self::Second
            | Self::Minute
            | Self::Hour
            | Self::Day
            | Self::Week
            | Self::Month
            | Self::Year
            | Self::NoUnits => value,

            // Pressure conversions → Pa
            Self::MillimetersWater => value * 9.80665, // mmH₂O → Pa
            Self::CentimetersWater => value * 98.0665, // cmH₂O → Pa
            Self::CentimetersOfMercury => value * 10.0, // cmHg → mmHg (approx)

            // Energy conversions → Wh
            Self::Joule => value / 3600.0,        // J → Wh
            Self::Kilojoule => value / 3.6,       // kJ → Wh
            Self::Btu => value * 0.293071,        // BTU → Wh
            Self::BtuPerHour => value * 0.293071, // BTU/h → W
            Self::Therm => value * 293071.0,      // therm → Wh
            Self::TonHour => value * 3517000.0,   // ton·h → Wh

            // Power conversion → W
            Self::Horsepower => value * 745.7, // hp → W

            // Flow conversions → L/min
            Self::GallonPerMinute => value * 3.785, // GPM → L/min
            Self::CubicFootPerMinute => value * 0.471947, // CFM → L/min
            Self::LiterPerSecond => value * 60.0,   // L/s → L/min

            // Volume conversions
            Self::CubicFootPerSecond => value * 0.0283168, // ft³/s → m³/s

            // Distance conversions (no major conversions needed in HA)
            // HA supports mm, cm, m, in, ft, mi, km directly

            // Mass conversions
            Self::MetricTon => value * 1000.0, // metric ton → kg
            Self::Ton => value * 907.185,      // short ton → kg (converted to lb in HA)
        };
        (converted, unit_str)
    }
}

impl From<EngineeringUnits> for u32 {
    fn from(u: EngineeringUnits) -> u32 {
        u.code()
    }
}

// ---------------------------------------------------------------------------
// Value conversion helpers (BACnet ↔ display/MQTT)
// ---------------------------------------------------------------------------

use crate::config::PointConfig;

/// Convert a raw BACnet value to a display/MQTT value using point configuration.
///
/// Applies scale+offset for numeric types, state text lookup for multi-state
/// objects, and pass-through for booleans and strings.
///
/// - `Real(f)` → `Real(f * scale + offset)`
/// - `SignedInt(n)` → `Real(n as f32 * scale + offset)`
/// - `UnsignedInt(n)` or `Enumerated(n)` with non-empty `state_text` and
///   `n >= 1`: look up `state_text[n-1]`; if found return `CharString(text)`;
///   otherwise apply numeric scaling.
/// - `Boolean` → pass-through.
/// - All other variants → pass-through.
pub fn convert_from_bacnet(value: &BacnetValue, config: &PointConfig) -> BacnetValue {
    let scale = config.scale;
    let offset = config.offset;

    match value {
        BacnetValue::Real(f) => BacnetValue::Real(f * scale + offset),

        BacnetValue::SignedInt(n) => BacnetValue::Real(*n as f32 * scale + offset),

        BacnetValue::UnsignedInt(n) => {
            if !config.state_text.is_empty() && *n >= 1 {
                let idx = (*n as usize) - 1;
                if let Some(label) = config.state_text.get(idx) {
                    let mut s = String::<64>::new();
                    // label is a heapless::String<16>; copy chars into String<64>
                    for ch in label.chars() {
                        // push() fails only when full; labels are ≤16 chars, target is 64
                        let _ = s.push(ch);
                    }
                    return BacnetValue::CharString(s);
                }
            }
            // No text mapping — apply numeric scaling
            BacnetValue::Real(*n as f32 * scale + offset)
        }

        BacnetValue::Enumerated(n) => {
            if !config.state_text.is_empty() && *n >= 1 {
                let idx = (*n as usize) - 1;
                if let Some(label) = config.state_text.get(idx) {
                    let mut s = String::<64>::new();
                    for ch in label.chars() {
                        let _ = s.push(ch);
                    }
                    return BacnetValue::CharString(s);
                }
            }
            // No text mapping — apply numeric scaling
            BacnetValue::Real(*n as f32 * scale + offset)
        }

        // Pass-through
        BacnetValue::Boolean(_)
        | BacnetValue::CharString(_)
        | BacnetValue::Null
        | BacnetValue::ObjectIdentifier(_) => value.clone(),
    }
}

/// Convert a user/MQTT string value back to a raw BACnet value.
///
/// Resolution order:
/// 1. If `state_text` is non-empty, check whether `display_value` matches
///    any label (case-sensitive). If found, return `Enumerated(index + 1)`.
/// 2. Try to parse as a number. Reverse scale/offset: `raw = (parsed - offset) / scale`.
///    Returns `Real(raw)`.
/// 3. If the string is one of "true", "Active", "ON", "1" → `Boolean(true)`.
///    If it is "false", "Inactive", "OFF", "0" → `Boolean(false)`.
/// 4. Otherwise return `None`.
pub fn convert_to_bacnet(display_value: &str, config: &PointConfig) -> Option<BacnetValue> {
    // 1. State text lookup (case-sensitive) — checked first so named states win.
    if !config.state_text.is_empty() {
        for (idx, label) in config.state_text.iter().enumerate() {
            if label.as_str() == display_value {
                return Some(BacnetValue::Enumerated(idx as u32 + 1));
            }
        }
    }

    // 2. Boolean strings — checked before numeric so "true"/"false" and "1"/"0"
    //    are treated as boolean when there is no state-text to consume them.
    match display_value {
        "true" | "Active" | "ON" => return Some(BacnetValue::Boolean(true)),
        "false" | "Inactive" | "OFF" => return Some(BacnetValue::Boolean(false)),
        _ => {}
    }

    // 3. Numeric: reverse scale/offset.  "1" and "0" also fall through here.
    if let Ok(parsed) = display_value.parse::<f32>() {
        // Special case: integer "1" → Boolean(true), "0" → Boolean(false).
        // This matches the common MQTT convention while still allowing higher
        // numeric writes (e.g. "42") to flow through as Real values.
        if display_value == "1" {
            return Some(BacnetValue::Boolean(true));
        }
        if display_value == "0" {
            return Some(BacnetValue::Boolean(false));
        }
        let scale = config.scale;
        let raw = if scale == 0.0 {
            parsed
        } else {
            (parsed - config.offset) / scale
        };
        return Some(BacnetValue::Real(raw));
    }

    None
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

    // =====================================================================
    // EngineeringUnits tests
    // =====================================================================

    #[test]
    fn engineering_units_from_code_temperature() {
        assert_eq!(
            EngineeringUnits::from_code(0),
            Some(EngineeringUnits::DegreesCelsius)
        );
        assert_eq!(
            EngineeringUnits::from_code(1),
            Some(EngineeringUnits::DegreesFahrenheit)
        );
        assert_eq!(
            EngineeringUnits::from_code(2),
            Some(EngineeringUnits::Kelvin)
        );
    }

    #[test]
    fn engineering_units_from_code_pressure() {
        assert_eq!(
            EngineeringUnits::from_code(3),
            Some(EngineeringUnits::Pascal)
        );
        assert_eq!(
            EngineeringUnits::from_code(4),
            Some(EngineeringUnits::Kilopascal)
        );
        assert_eq!(EngineeringUnits::from_code(6), Some(EngineeringUnits::Psi));
        assert_eq!(
            EngineeringUnits::from_code(7),
            Some(EngineeringUnits::MillimetersWater)
        );
    }

    #[test]
    fn engineering_units_from_code_energy_power() {
        assert_eq!(
            EngineeringUnits::from_code(19),
            Some(EngineeringUnits::Watt)
        );
        assert_eq!(
            EngineeringUnits::from_code(20),
            Some(EngineeringUnits::Kilowatt)
        );
        assert_eq!(
            EngineeringUnits::from_code(14),
            Some(EngineeringUnits::KilowattHour)
        );
        assert_eq!(
            EngineeringUnits::from_code(80),
            Some(EngineeringUnits::Horsepower)
        );
    }

    #[test]
    fn engineering_units_from_code_flow() {
        assert_eq!(
            EngineeringUnits::from_code(28),
            Some(EngineeringUnits::GallonPerMinute)
        );
        assert_eq!(
            EngineeringUnits::from_code(30),
            Some(EngineeringUnits::LiterPerMinute)
        );
    }

    #[test]
    fn engineering_units_from_code_distance() {
        assert_eq!(
            EngineeringUnits::from_code(32),
            Some(EngineeringUnits::Millimeter)
        );
        assert_eq!(
            EngineeringUnits::from_code(34),
            Some(EngineeringUnits::Meter)
        );
        assert_eq!(
            EngineeringUnits::from_code(35),
            Some(EngineeringUnits::Inch)
        );
        assert_eq!(
            EngineeringUnits::from_code(36),
            Some(EngineeringUnits::Foot)
        );
    }

    #[test]
    fn engineering_units_from_code_mass() {
        assert_eq!(
            EngineeringUnits::from_code(42),
            Some(EngineeringUnits::Kilogram)
        );
        assert_eq!(
            EngineeringUnits::from_code(44),
            Some(EngineeringUnits::Pound)
        );
    }

    #[test]
    fn engineering_units_from_code_electrical() {
        assert_eq!(
            EngineeringUnits::from_code(58),
            Some(EngineeringUnits::Ampere)
        );
        assert_eq!(
            EngineeringUnits::from_code(60),
            Some(EngineeringUnits::Volt)
        );
        assert_eq!(
            EngineeringUnits::from_code(65),
            Some(EngineeringUnits::Hertz)
        );
    }

    #[test]
    fn engineering_units_from_code_time() {
        assert_eq!(
            EngineeringUnits::from_code(69),
            Some(EngineeringUnits::Second)
        );
        assert_eq!(
            EngineeringUnits::from_code(70),
            Some(EngineeringUnits::Minute)
        );
        assert_eq!(
            EngineeringUnits::from_code(75),
            Some(EngineeringUnits::Hour)
        );
    }

    #[test]
    fn engineering_units_from_code_dimensionless() {
        assert_eq!(
            EngineeringUnits::from_code(95),
            Some(EngineeringUnits::Percent)
        );
        assert_eq!(EngineeringUnits::from_code(97), Some(EngineeringUnits::Ppm));
        assert_eq!(
            EngineeringUnits::from_code(123),
            Some(EngineeringUnits::Rpm)
        );
    }

    #[test]
    fn engineering_units_from_code_unknown() {
        assert_eq!(EngineeringUnits::from_code(9999), None);
        assert_eq!(EngineeringUnits::from_code(31), None);
        assert_eq!(EngineeringUnits::from_code(255), None);
    }

    #[test]
    fn engineering_units_round_trip() {
        let units = [
            EngineeringUnits::DegreesCelsius,
            EngineeringUnits::DegreesFahrenheit,
            EngineeringUnits::Pascal,
            EngineeringUnits::Kilopascal,
            EngineeringUnits::Psi,
            EngineeringUnits::Watt,
            EngineeringUnits::Kilowatt,
            EngineeringUnits::KilowattHour,
            EngineeringUnits::Volt,
            EngineeringUnits::Ampere,
            EngineeringUnits::Meter,
            EngineeringUnits::Foot,
            EngineeringUnits::Kilogram,
            EngineeringUnits::Pound,
            EngineeringUnits::Percent,
            EngineeringUnits::Horsepower,
            EngineeringUnits::GallonPerMinute,
            EngineeringUnits::MillimetersWater,
        ];
        for unit in units {
            let code = unit.code();
            let recovered = EngineeringUnits::from_code(code)
                .unwrap_or_else(|| panic!("Failed to recover unit from code {}", code));
            assert_eq!(unit, recovered, "Round-trip failed for {:?}", unit);
        }
    }

    #[test]
    fn engineering_units_ha_unit_str_passthrough() {
        // HA units that pass through unchanged
        assert_eq!(EngineeringUnits::DegreesCelsius.ha_unit_str(), "°C");
        assert_eq!(EngineeringUnits::DegreesFahrenheit.ha_unit_str(), "°F");
        assert_eq!(EngineeringUnits::Kelvin.ha_unit_str(), "K");
        assert_eq!(EngineeringUnits::Pascal.ha_unit_str(), "Pa");
        assert_eq!(EngineeringUnits::Kilopascal.ha_unit_str(), "kPa");
        assert_eq!(EngineeringUnits::Bar.ha_unit_str(), "bar");
        assert_eq!(EngineeringUnits::Psi.ha_unit_str(), "psi");
        assert_eq!(EngineeringUnits::Watt.ha_unit_str(), "W");
        assert_eq!(EngineeringUnits::Kilowatt.ha_unit_str(), "kW");
        assert_eq!(EngineeringUnits::Megawatt.ha_unit_str(), "MW");
        assert_eq!(EngineeringUnits::KilowattHour.ha_unit_str(), "kWh");
        assert_eq!(EngineeringUnits::Volt.ha_unit_str(), "V");
        assert_eq!(EngineeringUnits::Ampere.ha_unit_str(), "A");
        assert_eq!(EngineeringUnits::Meter.ha_unit_str(), "m");
        assert_eq!(EngineeringUnits::Foot.ha_unit_str(), "ft");
        assert_eq!(EngineeringUnits::Kilogram.ha_unit_str(), "kg");
        assert_eq!(EngineeringUnits::Pound.ha_unit_str(), "lb");
        assert_eq!(EngineeringUnits::Liter.ha_unit_str(), "L");
        assert_eq!(EngineeringUnits::Percent.ha_unit_str(), "%");
        assert_eq!(EngineeringUnits::Hertz.ha_unit_str(), "Hz");
    }

    #[test]
    fn engineering_units_ha_unit_str_conversions() {
        // BACnet units that require conversion
        assert_eq!(EngineeringUnits::MillimetersWater.ha_unit_str(), "Pa");
        assert_eq!(EngineeringUnits::CentimetersWater.ha_unit_str(), "Pa");
        assert_eq!(EngineeringUnits::Horsepower.ha_unit_str(), "W");
        assert_eq!(EngineeringUnits::GallonPerMinute.ha_unit_str(), "L/min");
        assert_eq!(EngineeringUnits::CubicFootPerMinute.ha_unit_str(), "L/min");
        assert_eq!(EngineeringUnits::Joule.ha_unit_str(), "Wh");
        assert_eq!(EngineeringUnits::Kilojoule.ha_unit_str(), "Wh");
        assert_eq!(EngineeringUnits::Btu.ha_unit_str(), "Wh");
        assert_eq!(EngineeringUnits::BtuPerHour.ha_unit_str(), "W");
    }

    #[test]
    fn engineering_units_convert_for_ha_passthrough() {
        // Units that pass through unchanged
        let (val, unit) = EngineeringUnits::DegreesCelsius.convert_for_ha(25.0);
        assert_eq!(val, 25.0);
        assert_eq!(unit, "°C");

        let (val, unit) = EngineeringUnits::DegreesFahrenheit.convert_for_ha(77.0);
        assert_eq!(val, 77.0);
        assert_eq!(unit, "°F");

        let (val, unit) = EngineeringUnits::Percent.convert_for_ha(50.0);
        assert_eq!(val, 50.0);
        assert_eq!(unit, "%");

        let (val, unit) = EngineeringUnits::Kilowatt.convert_for_ha(12.5);
        assert_eq!(val, 12.5);
        assert_eq!(unit, "kW");

        let (val, unit) = EngineeringUnits::Psi.convert_for_ha(30.0);
        assert_eq!(val, 30.0);
        assert_eq!(unit, "psi");

        let (val, unit) = EngineeringUnits::Foot.convert_for_ha(10.0);
        assert_eq!(val, 10.0);
        assert_eq!(unit, "ft");

        let (val, unit) = EngineeringUnits::Pound.convert_for_ha(150.0);
        assert_eq!(val, 150.0);
        assert_eq!(unit, "lb");
    }

    #[test]
    fn engineering_units_convert_for_ha_mmh2o_to_pa() {
        let (val, unit) = EngineeringUnits::MillimetersWater.convert_for_ha(100.0);
        assert!((val - 980.665).abs() < 0.01, "mmH₂O conversion failed");
        assert_eq!(unit, "Pa");
    }

    #[test]
    fn engineering_units_convert_for_ha_cmh2o_to_pa() {
        let (val, unit) = EngineeringUnits::CentimetersWater.convert_for_ha(10.0);
        assert!((val - 980.665).abs() < 0.01, "cmH₂O conversion failed");
        assert_eq!(unit, "Pa");
    }

    #[test]
    fn engineering_units_convert_for_ha_hp_to_w() {
        let (val, unit) = EngineeringUnits::Horsepower.convert_for_ha(1.0);
        assert!((val - 745.7).abs() < 0.1, "hp to W conversion failed");
        assert_eq!(unit, "W");

        let (val, unit) = EngineeringUnits::Horsepower.convert_for_ha(10.0);
        assert!((val - 7457.0).abs() < 0.1, "hp to W conversion failed");
        assert_eq!(unit, "W");
    }

    #[test]
    fn engineering_units_convert_for_ha_gpm_to_lmin() {
        let (val, unit) = EngineeringUnits::GallonPerMinute.convert_for_ha(10.0);
        assert!((val - 37.85).abs() < 0.01, "GPM to L/min conversion failed");
        assert_eq!(unit, "L/min");

        let (val, unit) = EngineeringUnits::GallonPerMinute.convert_for_ha(1.0);
        assert!((val - 3.785).abs() < 0.01, "GPM to L/min conversion failed");
        assert_eq!(unit, "L/min");
    }

    #[test]
    fn engineering_units_convert_for_ha_joule_to_wh() {
        let (val, unit) = EngineeringUnits::Joule.convert_for_ha(3600.0);
        assert!((val - 1.0).abs() < 0.01, "J to Wh conversion failed");
        assert_eq!(unit, "Wh");
    }

    #[test]
    fn engineering_units_convert_for_ha_kilojoule_to_wh() {
        let (val, unit) = EngineeringUnits::Kilojoule.convert_for_ha(3.6);
        assert!((val - 1.0).abs() < 0.01, "kJ to Wh conversion failed");
        assert_eq!(unit, "Wh");
    }

    #[test]
    fn engineering_units_convert_for_ha_btu_to_wh() {
        let (val, unit) = EngineeringUnits::Btu.convert_for_ha(1.0);
        assert!(
            (val - 0.293071).abs() < 0.001,
            "BTU to Wh conversion failed"
        );
        assert_eq!(unit, "Wh");
    }

    #[test]
    fn engineering_units_convert_for_ha_cfm_to_lmin() {
        let (val, unit) = EngineeringUnits::CubicFootPerMinute.convert_for_ha(100.0);
        assert!(
            (val - 47.1947).abs() < 0.01,
            "CFM to L/min conversion failed"
        );
        assert_eq!(unit, "L/min");
    }

    #[test]
    fn engineering_units_convert_for_ha_lps_to_lmin() {
        let (val, unit) = EngineeringUnits::LiterPerSecond.convert_for_ha(1.0);
        assert!((val - 60.0).abs() < 0.01, "L/s to L/min conversion failed");
        assert_eq!(unit, "L/min");
    }

    #[test]
    fn engineering_units_convert_for_ha_metric_ton_to_kg() {
        let (val, unit) = EngineeringUnits::MetricTon.convert_for_ha(1.0);
        assert!(
            (val - 1000.0).abs() < 0.01,
            "metric ton to kg conversion failed"
        );
        assert_eq!(unit, "kg");
    }

    #[test]
    fn engineering_units_unknown_code_returns_none() {
        assert_eq!(EngineeringUnits::from_code(9999), None);
        assert_eq!(EngineeringUnits::from_code(200), None);
        assert_eq!(EngineeringUnits::from_code(100), None);
    }

    #[test]
    fn engineering_units_no_units_variant() {
        assert_eq!(EngineeringUnits::NoUnits.ha_unit_str(), "");
        let (val, unit) = EngineeringUnits::NoUnits.convert_for_ha(42.0);
        assert_eq!(val, 42.0);
        assert_eq!(unit, "");
    }

    #[test]
    fn engineering_units_from_into_u32() {
        let unit = EngineeringUnits::Watt;
        let code: u32 = unit.into();
        assert_eq!(code, 19);

        let recovered = EngineeringUnits::from_code(code).unwrap();
        assert_eq!(recovered, EngineeringUnits::Watt);
    }

    #[test]
    fn engineering_units_comprehensive_coverage() {
        // Test a diverse set of units to ensure comprehensive coverage
        let test_cases = [
            (0, EngineeringUnits::DegreesCelsius, "°C"),
            (1, EngineeringUnits::DegreesFahrenheit, "°F"),
            (3, EngineeringUnits::Pascal, "Pa"),
            (6, EngineeringUnits::Psi, "psi"),
            (7, EngineeringUnits::MillimetersWater, "Pa"),
            (14, EngineeringUnits::KilowattHour, "kWh"),
            (19, EngineeringUnits::Watt, "W"),
            (20, EngineeringUnits::Kilowatt, "kW"),
            (28, EngineeringUnits::GallonPerMinute, "L/min"),
            (34, EngineeringUnits::Meter, "m"),
            (36, EngineeringUnits::Foot, "ft"),
            (42, EngineeringUnits::Kilogram, "kg"),
            (44, EngineeringUnits::Pound, "lb"),
            (51, EngineeringUnits::Liter, "L"),
            (58, EngineeringUnits::Ampere, "A"),
            (60, EngineeringUnits::Volt, "V"),
            (65, EngineeringUnits::Hertz, "Hz"),
            (75, EngineeringUnits::Hour, "h"),
            (95, EngineeringUnits::Percent, "%"),
        ];

        for (code, expected_unit, expected_str) in test_cases {
            let from_code = EngineeringUnits::from_code(code)
                .unwrap_or_else(|| panic!("Failed to decode code {}", code));
            assert_eq!(from_code, expected_unit);
            assert_eq!(from_code.code(), code);
            assert_eq!(from_code.ha_unit_str(), expected_str);
        }
    }

    // =====================================================================
    // convert_from_bacnet / convert_to_bacnet tests
    // =====================================================================

    use crate::config::PointConfig;
    use heapless::Vec as HVec;

    /// Build a PointConfig with given scale/offset and no state text.
    fn numeric_cfg(scale: f32, offset: f32) -> PointConfig {
        PointConfig {
            scale,
            offset,
            ..PointConfig::default()
        }
    }

    /// Build a PointConfig whose state_text is populated with the given labels.
    fn state_cfg(labels: &[&str]) -> PointConfig {
        let mut state_text: HVec<heapless::String<16>, 16> = HVec::new();
        for &label in labels {
            let mut s = heapless::String::<16>::new();
            let _ = s.push_str(label);
            let _ = state_text.push(s);
        }
        PointConfig {
            state_text,
            ..PointConfig::default()
        }
    }

    #[test]
    fn test_convert_from_bacnet_real_with_scale() {
        let cfg = numeric_cfg(2.0, 10.0);
        let result = convert_from_bacnet(&BacnetValue::Real(5.0), &cfg);
        assert_eq!(result, BacnetValue::Real(20.0)); // 5*2+10
    }

    #[test]
    fn test_convert_from_bacnet_real_identity() {
        let cfg = numeric_cfg(1.0, 0.0);
        let result = convert_from_bacnet(&BacnetValue::Real(42.5), &cfg);
        assert_eq!(result, BacnetValue::Real(42.5));
    }

    #[test]
    fn test_convert_from_bacnet_enumerated_with_text() {
        let cfg = state_cfg(&["Off", "Heat", "Cool", "Auto"]);
        // State 2 → "Heat" (index 1)
        let result = convert_from_bacnet(&BacnetValue::Enumerated(2), &cfg);
        let mut expected = heapless::String::<64>::new();
        let _ = expected.push_str("Heat");
        assert_eq!(result, BacnetValue::CharString(expected));
    }

    #[test]
    fn test_convert_from_bacnet_enumerated_without_text() {
        // No state_text → numeric scaling applies
        let cfg = numeric_cfg(1.0, 0.0);
        let result = convert_from_bacnet(&BacnetValue::Enumerated(3), &cfg);
        assert_eq!(result, BacnetValue::Real(3.0));
    }

    #[test]
    fn test_convert_from_bacnet_enumerated_out_of_range() {
        // State 5 with only 4 labels → fall back to numeric
        let cfg = state_cfg(&["Off", "Heat", "Cool", "Auto"]);
        let result = convert_from_bacnet(&BacnetValue::Enumerated(5), &cfg);
        // Scale 1, offset 0 → Real(5.0)
        assert_eq!(result, BacnetValue::Real(5.0));
    }

    #[test]
    fn test_convert_from_bacnet_boolean_passthrough() {
        let cfg = numeric_cfg(10.0, 5.0);
        assert_eq!(
            convert_from_bacnet(&BacnetValue::Boolean(true), &cfg),
            BacnetValue::Boolean(true)
        );
        assert_eq!(
            convert_from_bacnet(&BacnetValue::Boolean(false), &cfg),
            BacnetValue::Boolean(false)
        );
    }

    #[test]
    fn test_convert_from_bacnet_unsigned_with_state_text() {
        let cfg = state_cfg(&["Manual", "Auto", "Override"]);
        // State 2 → "Auto"
        let result = convert_from_bacnet(&BacnetValue::UnsignedInt(2), &cfg);
        let mut expected = heapless::String::<64>::new();
        let _ = expected.push_str("Auto");
        assert_eq!(result, BacnetValue::CharString(expected));
    }

    #[test]
    fn test_convert_to_bacnet_text_to_state() {
        let cfg = state_cfg(&["Off", "Heat", "Cool", "Auto"]);
        // "Cool" is index 2, state 3
        let result = convert_to_bacnet("Cool", &cfg);
        assert_eq!(result, Some(BacnetValue::Enumerated(3)));
    }

    #[test]
    fn test_convert_to_bacnet_number_reverse_scale() {
        // display = raw*2+10  →  raw = (display-10)/2
        let cfg = numeric_cfg(2.0, 10.0);
        // display 20 → raw (20-10)/2 = 5
        let result = convert_to_bacnet("20", &cfg);
        match result {
            Some(BacnetValue::Real(v)) => assert!((v - 5.0).abs() < 1e-4, "expected 5.0 got {}", v),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn test_convert_to_bacnet_boolean_strings() {
        let cfg = numeric_cfg(1.0, 0.0);
        assert_eq!(
            convert_to_bacnet("true", &cfg),
            Some(BacnetValue::Boolean(true))
        );
        assert_eq!(
            convert_to_bacnet("Active", &cfg),
            Some(BacnetValue::Boolean(true))
        );
        assert_eq!(
            convert_to_bacnet("ON", &cfg),
            Some(BacnetValue::Boolean(true))
        );
        assert_eq!(
            convert_to_bacnet("1", &cfg),
            Some(BacnetValue::Boolean(true))
        );
        assert_eq!(
            convert_to_bacnet("false", &cfg),
            Some(BacnetValue::Boolean(false))
        );
        assert_eq!(
            convert_to_bacnet("Inactive", &cfg),
            Some(BacnetValue::Boolean(false))
        );
        assert_eq!(
            convert_to_bacnet("OFF", &cfg),
            Some(BacnetValue::Boolean(false))
        );
        assert_eq!(
            convert_to_bacnet("0", &cfg),
            Some(BacnetValue::Boolean(false))
        );
    }

    #[test]
    fn test_convert_to_bacnet_invalid_returns_none() {
        let cfg = numeric_cfg(1.0, 0.0);
        assert_eq!(convert_to_bacnet("not-a-value", &cfg), None);
        assert_eq!(convert_to_bacnet("banana", &cfg), None);
    }

    #[test]
    fn test_roundtrip_numeric() {
        // raw = 7.5, scale = 0.5, offset = -5  → display = 7.5*0.5 + (-5) = -1.25
        // reverse: raw = (-1.25 - (-5)) / 0.5 = 3.75/0.5 = 7.5 ✓
        let cfg = numeric_cfg(0.5, -5.0);
        let original = BacnetValue::Real(7.5);
        let display = convert_from_bacnet(&original, &cfg);
        // Verify the forward conversion: 7.5 * 0.5 + (-5.0) = -1.25
        assert_eq!(display, BacnetValue::Real(-1.25_f32));
        // Now reverse using the known display string "-1.25"
        let back = convert_to_bacnet("-1.25", &cfg);
        match back {
            Some(BacnetValue::Real(v)) => {
                assert!((v - 7.5).abs() < 1e-4, "roundtrip failed: got {}", v)
            }
            other => panic!("expected Real(7.5), got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_state_text() {
        // "Heat" is state 2 in ["Off", "Heat", "Cool", "Auto"]
        let cfg = state_cfg(&["Off", "Heat", "Cool", "Auto"]);
        // Convert state 2 → display "Heat"
        let from = convert_from_bacnet(&BacnetValue::Enumerated(2), &cfg);
        let mut expected_str = heapless::String::<64>::new();
        let _ = expected_str.push_str("Heat");
        assert_eq!(from, BacnetValue::CharString(expected_str));
        // Convert "Heat" back → Enumerated(2)
        let back = convert_to_bacnet("Heat", &cfg);
        assert_eq!(back, Some(BacnetValue::Enumerated(2)));
    }

    // =====================================================================
    // Additional edge-case tests: convert_from_bacnet
    // =====================================================================

    #[test]
    fn test_convert_from_signed_int_with_scale() {
        // SignedInt(-10) scale=2 offset=5 → Real(-10 * 2 + 5) = Real(-15.0)
        let cfg = numeric_cfg(2.0, 5.0);
        let result = convert_from_bacnet(&BacnetValue::SignedInt(-10), &cfg);
        assert_eq!(result, BacnetValue::Real(-15.0));
    }

    #[test]
    fn test_convert_from_unsigned_zero_state() {
        // UnsignedInt(0) with state_text: 0 is below the 1-based range so
        // the state-text path is skipped and numeric scaling applies.
        let cfg = state_cfg(&["Off", "Heat", "Cool"]);
        // scale=1, offset=0 (state_cfg default) → Real(0.0)
        let result = convert_from_bacnet(&BacnetValue::UnsignedInt(0), &cfg);
        assert_eq!(result, BacnetValue::Real(0.0));
    }

    #[test]
    fn test_convert_from_null_passthrough() {
        let cfg = numeric_cfg(2.0, 100.0);
        let result = convert_from_bacnet(&BacnetValue::Null, &cfg);
        assert_eq!(result, BacnetValue::Null);
    }

    #[test]
    fn test_convert_from_charstring_passthrough() {
        // CharString is passed through unchanged even when scale/offset are set.
        let cfg = numeric_cfg(3.0, 7.0);
        let mut s = heapless::String::<64>::new();
        let _ = s.push_str("hello");
        let result = convert_from_bacnet(&BacnetValue::CharString(s.clone()), &cfg);
        assert_eq!(result, BacnetValue::CharString(s));
    }

    #[test]
    fn test_convert_from_real_negative_offset() {
        // Real(100) scale=1 offset=-32 → Real(100*1 + (-32)) = Real(68)
        let cfg = numeric_cfg(1.0, -32.0);
        let result = convert_from_bacnet(&BacnetValue::Real(100.0), &cfg);
        assert_eq!(result, BacnetValue::Real(68.0));
    }

    #[test]
    fn test_convert_from_real_zero_scale() {
        // Real(50) scale=0 offset=10 → Real(50*0 + 10) = Real(10)
        let cfg = numeric_cfg(0.0, 10.0);
        let result = convert_from_bacnet(&BacnetValue::Real(50.0), &cfg);
        assert_eq!(result, BacnetValue::Real(10.0));
    }

    // =====================================================================
    // Additional edge-case tests: convert_to_bacnet
    // =====================================================================

    #[test]
    fn test_convert_to_bacnet_case_sensitive() {
        // "heat" (lowercase) must NOT match the label "Heat" (title-case).
        let cfg = state_cfg(&["Off", "Heat", "Cool", "Auto"]);
        // State lookup fails, then "heat" doesn't parse as a number or boolean
        // keyword, so the result is None.
        assert_eq!(convert_to_bacnet("heat", &cfg), None);
    }

    #[test]
    fn test_convert_to_bacnet_negative_number() {
        // display="-25.5" with scale=0.5, offset=5
        // raw = (-25.5 - 5) / 0.5 = -30.5 / 0.5 = -61.0
        let cfg = numeric_cfg(0.5, 5.0);
        match convert_to_bacnet("-25.5", &cfg) {
            Some(BacnetValue::Real(v)) => {
                assert!((v - (-61.0_f32)).abs() < 1e-3, "expected -61.0, got {}", v)
            }
            other => panic!("expected Real(-61.0), got {:?}", other),
        }
    }

    #[test]
    fn test_convert_to_bacnet_zero_scale() {
        // scale=0 must not cause a division-by-zero; the implementation
        // returns the parsed value unchanged when scale==0.
        let cfg = numeric_cfg(0.0, 10.0);
        match convert_to_bacnet("42", &cfg) {
            // "42" parses as a number; not "1" or "0" → Real(42.0) unchanged
            Some(BacnetValue::Real(v)) => {
                assert!((v - 42.0_f32).abs() < 1e-4, "expected 42.0, got {}", v)
            }
            other => panic!("expected Real(42.0), got {:?}", other),
        }
    }

    #[test]
    fn test_convert_to_bacnet_empty_string() {
        // An empty string cannot match any label, boolean keyword, or number.
        let cfg = numeric_cfg(1.0, 0.0);
        assert_eq!(convert_to_bacnet("", &cfg), None);
    }

    #[test]
    fn test_convert_to_bacnet_state_priority() {
        // state_text contains "42" as a label. The label lookup runs first
        // (before the numeric parser), so "42" should match state 2, NOT
        // parse as a number.
        let cfg = state_cfg(&["10", "42", "99"]);
        // "42" is index 1 → Enumerated(2)
        assert_eq!(
            convert_to_bacnet("42", &cfg),
            Some(BacnetValue::Enumerated(2))
        );
    }

    // =====================================================================
    // Roundtrip tests
    // =====================================================================

    #[test]
    fn test_roundtrip_boolean() {
        // true → display "Active" (via the UI/HTTP layer which uses "Active"/"Inactive");
        // convert_to_bacnet("Active", ...) → Boolean(true).
        let cfg = numeric_cfg(1.0, 0.0);
        // Forward: Boolean passes through unchanged.
        let display = convert_from_bacnet(&BacnetValue::Boolean(true), &cfg);
        assert_eq!(display, BacnetValue::Boolean(true));
        // Reverse: user types "Active" → Boolean(true).
        let back = convert_to_bacnet("Active", &cfg);
        assert_eq!(back, Some(BacnetValue::Boolean(true)));
    }

    #[test]
    fn test_roundtrip_negative_scale() {
        // scale=-1 offset=100: display = raw * -1 + 100
        // raw=30 → display=70;  reverse: raw = (70 - 100) / -1 = 30 ✓
        let cfg = numeric_cfg(-1.0, 100.0);
        let display = convert_from_bacnet(&BacnetValue::Real(30.0), &cfg);
        assert_eq!(display, BacnetValue::Real(70.0));
        match convert_to_bacnet("70", &cfg) {
            Some(BacnetValue::Real(v)) => {
                assert!((v - 30.0_f32).abs() < 1e-4, "expected 30.0, got {}", v)
            }
            other => panic!("expected Real(30.0), got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_all_states() {
        let labels = ["Off", "Heat", "Cool", "Auto"];
        let cfg = state_cfg(&labels);
        for (i, &label) in labels.iter().enumerate() {
            let state = (i + 1) as u32; // BACnet is 1-based
                                        // Forward: Enumerated(state) → CharString(label)
            let display = convert_from_bacnet(&BacnetValue::Enumerated(state), &cfg);
            let mut expected = heapless::String::<64>::new();
            let _ = expected.push_str(label);
            assert_eq!(
                display,
                BacnetValue::CharString(expected),
                "forward failed for state {}",
                state
            );
            // Reverse: label → Enumerated(state)
            let back = convert_to_bacnet(label, &cfg);
            assert_eq!(
                back,
                Some(BacnetValue::Enumerated(state)),
                "reverse failed for label {}",
                label
            );
        }
    }

    // =====================================================================
    // PointConfig tests
    // =====================================================================

    #[test]
    fn test_point_config_default() {
        let cfg = PointConfig::default();
        assert_eq!(cfg.object_type, 0);
        assert_eq!(cfg.object_instance, 0);
        assert!(
            (cfg.scale - 1.0).abs() < f32::EPSILON,
            "default scale should be 1.0"
        );
        assert!(
            (cfg.offset - 0.0).abs() < f32::EPSILON,
            "default offset should be 0.0"
        );
        assert_eq!(cfg.engineering_unit, 95);
        assert!(
            cfg.bridge_to_bacnet_ip,
            "bridge_to_bacnet_ip default should be true"
        );
        assert!(cfg.bridge_to_mqtt, "bridge_to_mqtt default should be true");
        assert!(
            cfg.state_text.is_empty(),
            "state_text default should be empty"
        );
    }

    #[test]
    fn test_point_config_serde_roundtrip() {
        // Build a config with state_text populated.
        let cfg = PointConfig {
            object_type: 13, // MultiStateInput
            object_instance: 5,
            scale: 1.0,
            offset: 0.0,
            engineering_unit: 95,
            bridge_to_bacnet_ip: true,
            bridge_to_mqtt: false,
            state_text: {
                let mut v: heapless::Vec<heapless::String<16>, 16> = heapless::Vec::new();
                for label in &["Off", "Heat", "Cool", "Auto"] {
                    let mut s = heapless::String::<16>::new();
                    let _ = s.push_str(label);
                    let _ = v.push(s);
                }
                v
            },
        };
        let mut buf = [0u8; 512];
        let len = serde_json_core::to_slice(&cfg, &mut buf).expect("serialize failed");
        let (recovered, _): (PointConfig, _) =
            serde_json_core::from_slice(&buf[..len]).expect("deserialize failed");
        assert_eq!(recovered.object_type, 13);
        assert_eq!(recovered.object_instance, 5);
        assert!(!recovered.bridge_to_mqtt);
        assert_eq!(recovered.state_text.len(), 4);
        assert_eq!(recovered.state_text[0].as_str(), "Off");
        assert_eq!(recovered.state_text[3].as_str(), "Auto");
    }
}
