use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nack {
    IncorrectPatternFormat,
    UnknownPayloadType,
    MessageTooLarge,
    OutOfMemory,
    InvalidPayloadLength,
    Reserved(u8),
}

impl From<u8> for Nack {
    fn from(value: u8) -> Self {
        use Nack::*;
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

impl From<Nack> for u8 {
    fn from(value: Nack) -> Self {
        use Nack::*;
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
impl Nack {
    pub fn read<T: Read>(reader: &mut T) -> Nack {
        reader.read_u8().unwrap().into()
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_u8((*self).into()).unwrap();
    }
}
