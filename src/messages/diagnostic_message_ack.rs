use core::fmt;
use std::{
    fmt::Debug,
    io::{Read, Write},
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::logical_address::LogicalAddress;

use super::message_error::MessageError;

/// Diagnostic Acknowledgement Codes
///
/// These codes are used to indicate the status of a diagnostic message
/// sent from a tester to a DoIP server.
///
/// Contains both positive and negative acknowledgement codes.
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
#[repr(u8)]
#[non_exhaustive]
pub enum DiagnosticAckCode {
    /// Positive acknowledgement only
    RoutingConfirmationAck = 0x00,
    /// 0x01 is reserved for both positive and negative acknowledgements
    Reserved(u8),
    /// Negative acknowledgements
    InvalidSourceAddress = 0x02,
    UnknownTargetAddress = 0x03,
    DiagnosticMessageTooLarge = 0x04,
    OutOfMemory = 0x05,
    TargetUnreachable = 0x06,
    UnknownNetwork = 0x07,
    TransportProtocolError = 0x08,
}

impl DiagnosticAckCode {
    /// Returns true if the code is a positive acknowledgement
    pub fn is_positive_ack(&self) -> bool {
        matches!(self, DiagnosticAckCode::RoutingConfirmationAck)
    }

    /// Returns true if the code is a negative acknowledgement
    pub fn is_negative_ack(&self) -> bool {
        !self.is_positive_ack()
    }
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

impl Debug for DiagnosticAckCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:#04X})", self, u8::from(*self))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DiagnosticMessageAck {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: Vec<u8>,
}

impl DiagnosticMessageAck {
    /// Deserialize a diagnostic message acknowledgement from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
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

    /// Serialize this diagnostic message acknowledgement to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print() {
        let ack = DiagnosticMessageAck {
            source_address: LogicalAddress(0x1234),
            target_address: LogicalAddress(0x5678),
            ack_code: DiagnosticAckCode::Reserved(8),
            previous_message_data: vec![0x01, 0x02, 0x03],
        };
        println!("{ack:?}");
    }
}
