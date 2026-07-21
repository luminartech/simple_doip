use crate::LogicalAddress;

use automotive_wire_codec::{read_u16_be, write_all, write_u16_be};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticMessage<'a> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: &'a [u8],
}

/// Owned mirror of [`DiagnosticMessage`] for values that must outlive an RX buffer
/// (tokio channels, `ServerConnectionHandler` responses).
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedDiagnosticMessage {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl OwnedDiagnosticMessage {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> DiagnosticMessage<'_> {
        DiagnosticMessage {
            source_address: self.source_address,
            target_address: self.target_address,
            user_data: &self.user_data,
        }
    }
}

#[cfg(feature = "alloc")]
impl DiagnosticMessage<'_> {
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedDiagnosticMessage {
        OwnedDiagnosticMessage {
            source_address: self.source_address,
            target_address: self.target_address,
            user_data: self.user_data.to_vec(),
        }
    }
}

impl<'a> Decode<'a> for DiagnosticMessage<'a> {
    type Error = MessageError;

    /// Deserialize a diagnostic message from a byte slice. Consumes the entire buffer;
    /// all bytes after the source/target addresses are treated as opaque user data.
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        let (target_address, rest) = read_u16_be(rest)?;

        // Remaining bytes are opaque user data; consume the whole buffer.
        let user_data = rest;

        Ok((
            Self {
                source_address: LogicalAddress(source_address),
                target_address: LogicalAddress(target_address),
                user_data,
            },
            &[],
        ))
    }
}

impl Encode for DiagnosticMessage<'_> {
    type Error = MessageError;

    /// Closed form matching [`Self::encode`]: 2-byte source + 2-byte target + user data.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(4 + self.user_data.len())
    }

    /// Serialize this diagnostic message into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        write_u16_be(writer, self.target_address.into())?;
        let user_data = self.user_data;
        write_all(writer, user_data)?;
        Ok(4 + user_data.len())
    }
}
