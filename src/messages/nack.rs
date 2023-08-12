use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegativeAck {
    IncorrectPatternFormat,
    UnknownPayloadType,
    MessageTooLarge,
    OutOfMemory,
    InvalidPayloadLength,
    Reserved(u8),
}

impl From<u8> for NegativeAck {
    fn from(value: u8) -> Self {
        use NegativeAck::*;
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

impl From<NegativeAck> for u8 {
    fn from(value: NegativeAck) -> Self {
        use NegativeAck::*;
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

/// DoIP Message Header
impl NegativeAck {
    pub fn read<T: Read>(reader: &mut T) -> NegativeAck {
        reader.read_u8().unwrap().into()
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        let raw: u8 = self.into();
        writer.write_u8(self.into()).unwrap();
    }
}
