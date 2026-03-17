/// Errors that can occur during encoding of BACnet/DNS packets.
#[derive(Debug, Clone, PartialEq)]
pub enum EncodeError {
    /// The output buffer is too small to hold the encoded data.
    BufferTooSmall,
    /// A string value is too long to encode.
    StringTooLong,
    /// An invalid value was provided (e.g. out-of-range instance number).
    InvalidValue,
}

/// Errors that can occur during decoding of BACnet/DNS packets.
#[derive(Debug, Clone, PartialEq)]
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

/// Umbrella error type for bridge operations.
#[derive(Debug, Clone, PartialEq)]
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
