use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::error::DoIPError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticPositiveAckCode {
    RoutingConfirmationAck,
    Reserved(u8),
}

impl From<u8> for DiagnosticPositiveAckCode {
    fn from(value: u8) -> Self {
        use DiagnosticPositiveAckCode::*;
        match value {
            0x00 => RoutingConfirmationAck,
            _ => Reserved(value),
        }
    }
}

impl From<DiagnosticPositiveAckCode> for u8 {
    fn from(value: DiagnosticPositiveAckCode) -> Self {
        use DiagnosticPositiveAckCode::*;
        match value {
            RoutingConfirmationAck => 0x00,
            Reserved(value) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessagePositiveAck {
    pub source_address: u16,
    pub target_address: u16,
    pub ack_code: DiagnosticPositiveAckCode,
    pub previous_message_data: Vec<u8>,
}

impl DiagnosticMessagePositiveAck {
    pub fn read<T: Read>(reader: &mut T, payload_length: u32) -> Result<Self, DoIPError> {
        let source_address = reader.read_u16::<BigEndian>()?;

        let target_address = reader.read_u16::<BigEndian>()?;
        let ack_code = reader.read_u8()?.into();
        let previous_message_data_len = payload_length - 5; // 4 == source + target address
        let mut previous_message_data = Vec::with_capacity(previous_message_data_len as usize);
        if previous_message_data_len > 0 {
            reader.read_exact(&mut previous_message_data)?;
        }

        Ok(Self {
            source_address,
            target_address,
            ack_code,
            previous_message_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPError> {
        writer.write_u16::<BigEndian>(self.source_address)?;
        writer.write_u16::<BigEndian>(self.target_address)?;
        writer.write_u8(self.ack_code.into())?;
        writer.write_all(&self.previous_message_data)?;
        Ok(())
    }
}
