use core::fmt;

use crate::logical_address::LogicalAddress;

use automotive_wire_codec::{read_u8, read_u16_be, write_all, write_u8, write_u16_be};

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

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DiagnosticMessageAck<'a> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: &'a [u8],
}

/// Owned mirror of [`DiagnosticMessageAck`] for values that must outlive an RX buffer
/// (tokio channels, `ServerConnectionHandler` responses).
#[cfg(feature = "alloc")]
#[derive(Clone, Eq, PartialEq)]
pub struct OwnedDiagnosticMessageAck {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl OwnedDiagnosticMessageAck {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> DiagnosticMessageAck<'_> {
        DiagnosticMessageAck {
            source_address: self.source_address,
            target_address: self.target_address,
            ack_code: self.ack_code,
            previous_message_data: &self.previous_message_data,
        }
    }
}

#[cfg(feature = "alloc")]
impl DiagnosticMessageAck<'_> {
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedDiagnosticMessageAck {
        OwnedDiagnosticMessageAck {
            source_address: self.source_address,
            target_address: self.target_address,
            ack_code: self.ack_code,
            previous_message_data: self.previous_message_data.to_vec(),
        }
    }
}

impl<'a> Decode<'a> for DiagnosticMessageAck<'a> {
    type Error = MessageError;

    /// Deserialize a diagnostic message acknowledgement from a byte slice. Consumes the
    /// entire buffer; all bytes after the fixed fields are treated as the previous
    /// message data.
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
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

impl Encode for DiagnosticMessageAck<'_> {
    type Error = MessageError;

    /// Serialize this diagnostic message acknowledgement into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        write_u16_be(writer, self.target_address.into())?;
        write_u8(writer, self.ack_code.into())?;
        let previous_message_data = self.previous_message_data;
        write_all(writer, previous_message_data)?;
        Ok(5 + previous_message_data.len())
    }
}

impl fmt::Debug for DiagnosticMessageAck<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let previous_message_data = self.previous_message_data;
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

#[cfg(feature = "alloc")]
impl fmt::Debug for OwnedDiagnosticMessageAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
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
