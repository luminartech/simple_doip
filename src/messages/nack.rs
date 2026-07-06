use super::decode_util::read_u8;
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// Negative Acknowledgement payload
/// Only sent by the server except in development
/// Indicates error condition in previously received message
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum NackCode {
    /// Protocol or inverse protocol version mismatch
    ///
    /// `DoIP` entity action: Close socket
    IncorrectPatternFormat = 0x00,

    /// Payload type is not supported by `DoIP` entity
    ///
    /// `DoIP` entity action: Discard `DoIP` message
    UnknownPayloadType = 0x01,

    /// Payload length (without header) is larger than maximum data size (MDS) supported by `DoIP` entity
    ///
    /// `DoIP` entity action: Discard `DoIP` message
    MessageTooLarge = 0x02,

    /// Payload length exceeds currently available `DoIP` protocol handler memory of the `DoIP` entity
    ///
    /// `DoIP` entity action: Discard `DoIP` message
    OutOfMemory = 0x03,

    /// Payload length param does not match the expected length for the specific payload type.
    /// Includes payload-type-specific min length, fixed length, and max length checks
    ///
    /// `DoIP` entity action: Close socket
    InvalidPayloadLength = 0x04,
    Reserved(u8),
}

impl From<u8> for NackCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => NackCode::IncorrectPatternFormat,
            0x01 => NackCode::UnknownPayloadType,
            0x02 => NackCode::MessageTooLarge,
            0x03 => NackCode::OutOfMemory,
            0x04 => NackCode::InvalidPayloadLength,
            _ => NackCode::Reserved(value),
        }
    }
}

impl From<NackCode> for u8 {
    fn from(value: NackCode) -> Self {
        match value {
            NackCode::IncorrectPatternFormat => 0x00,
            NackCode::UnknownPayloadType => 0x01,
            NackCode::MessageTooLarge => 0x02,
            NackCode::OutOfMemory => 0x03,
            NackCode::InvalidPayloadLength => 0x04,
            NackCode::Reserved(value) => value,
        }
    }
}

impl<'a> Decode<'a> for NackCode {
    /// Deserialize a negative acknowledgement code from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (value, rest) = read_u8(buf)?;
        Ok((value.into(), rest))
    }
}

impl Encode for NackCode {
    fn encoded_size(&self) -> usize {
        1
    }

    /// Serialize this negative acknowledgement code into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        writer
            .write_all(&[(*self).into()])
            .map_err(MessageError::io)?;
        Ok(1)
    }
}
