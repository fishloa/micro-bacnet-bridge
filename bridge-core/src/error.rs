/// Errors that can occur during encoding of BACnet/DNS packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// The output buffer is too small to hold the encoded data.
    BufferTooSmall,
    /// A string value is too long to encode.
    StringTooLong,
    /// An invalid value was provided (e.g. out-of-range instance number).
    InvalidValue,
}

// L3: Display implementation lets callers log errors without requiring `Debug`
// formatting, and is required by the `std::error::Error` trait on hosted targets.
impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::BufferTooSmall => f.write_str("encode error: buffer too small"),
            EncodeError::StringTooLong => f.write_str("encode error: string too long"),
            EncodeError::InvalidValue => f.write_str("encode error: invalid value"),
        }
    }
}

/// Errors that can occur during decoding of BACnet/DNS packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The input data is too short to contain a valid packet.
    UnexpectedEnd,
    /// A field contained an unrecognized or invalid value.
    InvalidData,
    /// The version field is not the expected value.
    InvalidVersion,
    /// A length field points past the end of the buffer.
    LengthOutOfBounds,
    /// A name pointer in a DNS packet causes a loop or points out of bounds.
    InvalidNamePointer,
}

// L3: Display implementation for DecodeError.
impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::UnexpectedEnd => f.write_str("decode error: unexpected end of data"),
            DecodeError::InvalidData => f.write_str("decode error: invalid data"),
            DecodeError::InvalidVersion => f.write_str("decode error: invalid version"),
            DecodeError::LengthOutOfBounds => f.write_str("decode error: length out of bounds"),
            DecodeError::InvalidNamePointer => {
                f.write_str("decode error: invalid DNS name pointer")
            }
        }
    }
}

/// Umbrella error type for bridge operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    Encode(EncodeError),
    Decode(DecodeError),
    /// Configuration is invalid or corrupt.
    InvalidConfig,
    /// A ring buffer operation failed (e.g. buffer full).
    RingBufferFull,
    /// An IPC channel is not initialised.
    IpcNotReady,
}

// L3: Display implementation for BridgeError.
impl core::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BridgeError::Encode(e) => write!(f, "bridge error: {}", e),
            BridgeError::Decode(e) => write!(f, "bridge error: {}", e),
            BridgeError::InvalidConfig => f.write_str("bridge error: invalid configuration"),
            BridgeError::RingBufferFull => f.write_str("bridge error: ring buffer full"),
            BridgeError::IpcNotReady => f.write_str("bridge error: IPC channel not ready"),
        }
    }
}

impl From<EncodeError> for BridgeError {
    fn from(e: EncodeError) -> Self {
        BridgeError::Encode(e)
    }
}

impl From<DecodeError> for BridgeError {
    fn from(e: DecodeError) -> Self {
        BridgeError::Decode(e)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // L3: Verify that Display produces non-empty, meaningful strings for every
    // variant.  We use a heapless::String as our write target since this crate
    // is no_std.
    fn display_str<T: core::fmt::Display>(val: &T) -> heapless::String<256> {
        use core::fmt::Write;
        let mut s: heapless::String<256> = heapless::String::new();
        write!(s, "{}", val).ok();
        s
    }

    #[test]
    fn encode_error_display() {
        assert!(display_str(&EncodeError::BufferTooSmall).contains("buffer too small"));
        assert!(display_str(&EncodeError::StringTooLong).contains("string too long"));
        assert!(display_str(&EncodeError::InvalidValue).contains("invalid value"));
    }

    #[test]
    fn decode_error_display() {
        assert!(display_str(&DecodeError::UnexpectedEnd).contains("unexpected end"));
        assert!(display_str(&DecodeError::InvalidData).contains("invalid data"));
        assert!(display_str(&DecodeError::InvalidVersion).contains("invalid version"));
        assert!(display_str(&DecodeError::LengthOutOfBounds).contains("length out of bounds"));
        assert!(display_str(&DecodeError::InvalidNamePointer).contains("name pointer"));
    }

    #[test]
    fn bridge_error_display() {
        let be = BridgeError::Encode(EncodeError::BufferTooSmall);
        assert!(display_str(&be).contains("buffer too small"));

        let bd = BridgeError::Decode(DecodeError::InvalidData);
        assert!(display_str(&bd).contains("invalid data"));

        assert!(display_str(&BridgeError::InvalidConfig).contains("invalid configuration"));
        assert!(display_str(&BridgeError::RingBufferFull).contains("ring buffer full"));
        assert!(display_str(&BridgeError::IpcNotReady).contains("IPC channel not ready"));
    }

    #[test]
    fn from_encode_error() {
        let be: BridgeError = EncodeError::StringTooLong.into();
        assert_eq!(be, BridgeError::Encode(EncodeError::StringTooLong));
    }

    #[test]
    fn from_decode_error() {
        let be: BridgeError = DecodeError::UnexpectedEnd.into();
        assert_eq!(be, BridgeError::Decode(DecodeError::UnexpectedEnd));
    }
}
