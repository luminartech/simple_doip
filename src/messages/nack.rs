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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_nack_code_roundtrip(byte in any::<u8>()) {
            let code = NackCode::from(byte);
            let back: u8 = code.into();
            prop_assert_eq!(byte, back);
        }

        #[test]
        fn prop_nack_serde_roundtrip(byte in any::<u8>()) {
            let code = NackCode::from(byte);
            let mut buf = Vec::new();
            code.write(&mut buf).unwrap();

            let parsed = NackCode::read(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(code, parsed);
        }
    }
}
