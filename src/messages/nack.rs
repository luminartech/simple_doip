use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

use super::message_error::MessageError;

/// Negative Acknowledgement payload
/// Only sent by the server except in development
/// Indicates error condition in previously received message
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum NackCode {
    /// Protocol or inverse protocol version mismatch
    ///
    /// DoIP entity action: Close socket
    IncorrectPatternFormat = 0x00,

    /// Payload type is not supported by DoIP entity
    ///
    /// DoIP entity action: Discard DoIP message
    UnknownPayloadType = 0x01,

    /// Payload length (without header) is larger than maximum data size (MDS) supported by DoIP entity
    ///
    /// DoIP entity action: Discard DoIP message
    MessageTooLarge = 0x02,

    /// Payload length exceeds currently available DoIP protocol handler memory of the DoIP entity
    ///
    /// DoIP entity action: Discard DoIP message
    OutOfMemory = 0x03,

    /// Payload length param does not match the expected length for the specific payload type.
    /// Includes payload-type-specific min length, fixed length, and max length checks
    ///
    /// DoIP entity action: Close socket
    InvalidPayloadLength = 0x04,
    Reserved(u8),
}

impl From<u8> for NackCode {
    fn from(value: u8) -> Self {
        use NackCode::*;
        match value {
            0x00 => IncorrectPatternFormat,
            0x01 => UnknownPayloadType,
            0x02 => MessageTooLarge,
            0x03 => OutOfMemory,
            0x04 => InvalidPayloadLength,
            _ => Reserved(value),
        }
    }
}

impl From<NackCode> for u8 {
    fn from(value: NackCode) -> Self {
        use NackCode::*;
        match value {
            IncorrectPatternFormat => 0x00,
            UnknownPayloadType => 0x01,
            MessageTooLarge => 0x02,
            OutOfMemory => 0x03,
            InvalidPayloadLength => 0x04,
            Reserved(value) => value,
        }
    }
}

impl NackCode {
    /// Deserialize a negative acknowledgement code from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        Ok(reader.read_u8()?.into())
    }
    /// Serialize this negative acknowledgement code to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u8((*self).into())?;
        Ok(1)
    }
}
