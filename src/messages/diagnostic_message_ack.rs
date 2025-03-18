use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use super::message_error::MessageError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticAckCode {
    RoutingConfirmationAck,
    Reserved(u8),
    InvalidSourceAddress,
    UnknownTargetAddress,
    DiagnosticMessageTooLarge,
    OutOfMemory,
    TargetUnreachable,
    UnknownNetwork,
    TransportProtocolError,
}

impl From<u8> for DiagnosticAckCode {
    fn from(value: u8) -> Self {
        use DiagnosticAckCode::*;
        match value {
            0x00 => RoutingConfirmationAck,
            0x01 => Reserved(value),
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

impl From<DiagnosticAckCode> for u8 {
    fn from(value: DiagnosticAckCode) -> Self {
        use DiagnosticAckCode::*;
        match value {
            RoutingConfirmationAck => 0x00,
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
pub struct DiagnosticMessageAck {
    pub source_address: u16,
    pub target_address: u16,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: Vec<u8>,
}

impl DiagnosticMessageAck {
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let source_address = reader.read_u16::<BigEndian>()?;

        let target_address = reader.read_u16::<BigEndian>()?;
        let ack_code = reader.read_u8()?.into();
        let mut previous_message_data = Vec::new();
        reader.read_to_end(&mut previous_message_data)?;

        Ok(Self {
            source_address,
            target_address,
            ack_code,
            previous_message_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.source_address)?;
        writer.write_u16::<BigEndian>(self.target_address)?;
        writer.write_u8(self.ack_code.into())?;
        writer.write_all(&self.previous_message_data)?;
        Ok(5 + self.previous_message_data.len())
    }
}
