use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::error::DoIPError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticNegativeAckCode {
    Reserved(u8),
    InvalidSourceAddress,
    UnknownTargetAddress,
    DiagnosticMessageTooLarge,
    OutOfMemory,
    TargetUnreachable,
    UnknownNetwork,
    TransportProtocolError,
}

impl From<u8> for DiagnosticNegativeAckCode {
    fn from(value: u8) -> Self {
        use DiagnosticNegativeAckCode::*;
        match value {
            0x00..=0x01 => Reserved(value),
            0x02 => InvalidSourceAddress,
            0x03 => UnknownTargetAddress,
            0x04 => DiagnosticMessageTooLarge,
            0x05 => OutOfMemory,
            0x06 => TargetUnreachable,
            0x07 => UnknownNetwork,
            0x08 => TransportProtocolError,
            0x09..=0xFF => Reserved(value),
        }
    }
}

impl From<DiagnosticNegativeAckCode> for u8 {
    fn from(value: DiagnosticNegativeAckCode) -> Self {
        use DiagnosticNegativeAckCode::*;
        match value {
            Reserved(value) => value,
            InvalidSourceAddress => 0x02,
            UnknownTargetAddress => 0x03,
            DiagnosticMessageTooLarge => 0x04,
            OutOfMemory => 0x05,
            TargetUnreachable => 0x06,
            UnknownNetwork => 0x07,
            TransportProtocolError => 0x08,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessageNegativeAck {
    pub source_address: u16,
    pub target_address: u16,
    pub nack_code: DiagnosticNegativeAckCode,
    pub previous_message_data: Vec<u8>,
}

impl DiagnosticMessageNegativeAck {
    pub fn read<T: Read>(reader: &mut T, payload_length: u32) -> Result<Self, DoIPError> {
        let source_address = reader.read_u16::<BigEndian>()?;

        let target_address = reader.read_u16::<BigEndian>()?;
        let nack_code = reader.read_u8()?.into();
        let previous_message_data_len = payload_length - 5; // 4 == source + target address
        let mut previous_message_data = Vec::with_capacity(previous_message_data_len as usize);
        if previous_message_data_len > 0 {
            reader.read_exact(&mut previous_message_data)?;
        }

        Ok(Self {
            source_address,
            target_address,
            nack_code,
            previous_message_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPError> {
        writer.write_u16::<BigEndian>(self.source_address)?;
        writer.write_u16::<BigEndian>(self.target_address)?;
        writer.write_u8(self.nack_code.into())?;
        writer.write_all(&self.previous_message_data)?;
        Ok(())
    }
}
