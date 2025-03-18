use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

use super::message_error::MessageError;

/// Negative Acknowledgement payload
/// Only sent by the server except in development
/// Indicates error condition in previously received message
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NackCode {
    IncorrectPatternFormat,
    UnknownPayloadType,
    MessageTooLarge,
    OutOfMemory,
    InvalidPayloadLength,
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

/// Nack read/write
impl NackCode {
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        Ok(reader.read_u8()?.into())
    }
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u8((*self).into())?;
        Ok(1)
    }
}
