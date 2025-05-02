use core::fmt;
use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::logical_address::LogicalAddress;

use super::message_error::MessageError;

/// Diagnostic Acknowledgement Codes
///
/// These codes are used to indicate the status of a diagnostic message
/// sent from a tester to a DoIP server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticAckCode {
    RoutingConfirmationAck = 0x00,
    Reserved(u8),
    InvalidSourceAddress = 0x02,
    UnknownTargetAddress = 0x03,
    DiagnosticMessageTooLarge = 0x04,
    OutOfMemory = 0x05,
    TargetUnreachable = 0x06,
    UnknownNetwork = 0x07,
    TransportProtocolError = 0x08,
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

#[derive(Clone, PartialEq, Eq)]
pub struct DiagnosticMessageAck {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: Vec<u8>,
}

impl DiagnosticMessageAck {
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let source_address = LogicalAddress(reader.read_u16::<BigEndian>()?);

        let target_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
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
        writer.write_u16::<BigEndian>(self.source_address.into())?;
        writer.write_u16::<BigEndian>(self.target_address.into())?;
        writer.write_u8(self.ack_code.into())?;
        writer.write_all(&self.previous_message_data)?;
        Ok(5 + self.previous_message_data.len())
    }
}

impl fmt::Debug for DiagnosticMessageAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticMessageAck")
            .field("source_address", &self.source_address)
            .field("target_address", &self.target_address)
            .field("ack_code", &self.ack_code)
            .field(
                "previous_message_data",
                &format_args!(
                    "({} bytes): {:#04X?}",
                    self.previous_message_data.len(),
                    self.previous_message_data
                ),
            )
            .finish()
    }
}
