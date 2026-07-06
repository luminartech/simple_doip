use core::fmt;

use crate::logical_address::LogicalAddress;

use super::decode_util::{read_u8, read_u16_be};
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// Diagnostic Acknowledgement Codes
///
/// These codes are used to indicate the status of a diagnostic message
/// sent from a tester to a `DoIP` server.
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
    #[must_use]
    pub fn is_positive_ack(&self) -> bool {
        matches!(self, DiagnosticAckCode::RoutingConfirmationAck)
    }

    /// Returns true if the code is a negative acknowledgement
    #[must_use]
    pub fn is_negative_ack(&self) -> bool {
        !self.is_positive_ack()
    }
}

impl From<u8> for DiagnosticAckCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => DiagnosticAckCode::RoutingConfirmationAck,
            0x02 => DiagnosticAckCode::InvalidSourceAddress,
            0x03 => DiagnosticAckCode::UnknownTargetAddress,
            0x04 => DiagnosticAckCode::DiagnosticMessageTooLarge,
            0x05 => DiagnosticAckCode::OutOfMemory,
            0x06 => DiagnosticAckCode::TargetUnreachable,
            0x07 => DiagnosticAckCode::UnknownNetwork,
            0x08 => DiagnosticAckCode::TransportProtocolError,
            _ => DiagnosticAckCode::Reserved(value),
        }
    }
}

impl From<DiagnosticAckCode> for u8 {
    fn from(value: DiagnosticAckCode) -> Self {
        match value {
            DiagnosticAckCode::RoutingConfirmationAck => 0x00,
            DiagnosticAckCode::Reserved(value) => value,
            DiagnosticAckCode::InvalidSourceAddress => 0x02,
            DiagnosticAckCode::UnknownTargetAddress => 0x03,
            DiagnosticAckCode::DiagnosticMessageTooLarge => 0x04,
            DiagnosticAckCode::OutOfMemory => 0x05,
            DiagnosticAckCode::TargetUnreachable => 0x06,
            DiagnosticAckCode::UnknownNetwork => 0x07,
            DiagnosticAckCode::TransportProtocolError => 0x08,
        }
    }
}

impl fmt::Debug for DiagnosticAckCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:#04X})", self, u8::from(*self))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DiagnosticMessageAck<D> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: D,
}

impl<'a> Decode<'a> for DiagnosticMessageAck<&'a [u8]> {
    /// Deserialize a diagnostic message acknowledgement from a byte slice. Consumes the
    /// entire buffer; all bytes after the fixed fields are treated as the previous
    /// message data.
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        let (target_address, rest) = read_u16_be(rest)?;
        let (ack_code, rest) = read_u8(rest)?;

        // Remaining bytes are the previous message data; consume the whole buffer.
        let previous_message_data = rest;

        Ok((
            Self {
                source_address: LogicalAddress(source_address),
                target_address: LogicalAddress(target_address),
                ack_code: ack_code.into(),
                previous_message_data,
            },
            &[],
        ))
    }
}

impl<D: AsRef<[u8]>> Encode for DiagnosticMessageAck<D> {
    fn encoded_size(&self) -> usize {
        5 + self.previous_message_data.as_ref().len()
    }

    /// Serialize this diagnostic message acknowledgement into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        writer
            .write_all(&u16::to_be_bytes(self.source_address.into()))
            .map_err(MessageError::io)?;
        writer
            .write_all(&u16::to_be_bytes(self.target_address.into()))
            .map_err(MessageError::io)?;
        writer
            .write_all(&[self.ack_code.into()])
            .map_err(MessageError::io)?;
        let previous_message_data = self.previous_message_data.as_ref();
        writer
            .write_all(previous_message_data)
            .map_err(MessageError::io)?;
        Ok(5 + previous_message_data.len())
    }
}

impl<D: AsRef<[u8]>> fmt::Debug for DiagnosticMessageAck<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let previous_message_data = self.previous_message_data.as_ref();
        f.debug_struct("DiagnosticMessageAck")
            .field("source_address", &self.source_address)
            .field("target_address", &self.target_address)
            .field("ack_code", &self.ack_code)
            .field(
                "previous_message_data",
                &format_args!(
                    "({} bytes): {:#04X?}",
                    previous_message_data.len(),
                    previous_message_data
                ),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write as _;

    /// Minimal fixed-capacity `core::fmt::Write` sink so this test does not depend on
    /// `std`/`alloc`.
    struct FixedBuf {
        buf: [u8; 256],
        len: usize,
    }

    impl core::fmt::Write for FixedBuf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let end = self.len + bytes.len();
            if end > self.buf.len() {
                return Err(core::fmt::Error);
            }
            self.buf[self.len..end].copy_from_slice(bytes);
            self.len = end;
            Ok(())
        }
    }

    #[test]
    fn test_print() {
        let ack = DiagnosticMessageAck {
            source_address: LogicalAddress(0x1234),
            target_address: LogicalAddress(0x5678),
            ack_code: DiagnosticAckCode::Reserved(8),
            previous_message_data: &[0x01, 0x02, 0x03][..],
        };
        let mut out = FixedBuf {
            buf: [0u8; 256],
            len: 0,
        };
        write!(out, "{ack:?}").unwrap();
        assert!(out.len > 0);
    }
}
