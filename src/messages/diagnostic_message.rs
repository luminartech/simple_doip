use crate::LogicalAddress;

use super::decode_util::read_u16_be;
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticMessage<D> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: D,
}

impl<'a> Decode<'a> for DiagnosticMessage<&'a [u8]> {
    /// Deserialize a diagnostic message from a byte slice. Consumes the entire buffer;
    /// all bytes after the source/target addresses are treated as opaque user data.
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
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

impl<D: AsRef<[u8]>> Encode for DiagnosticMessage<D> {
    fn encoded_size(&self) -> usize {
        4 + self.user_data.as_ref().len()
    }

    /// Serialize this diagnostic message into `writer`
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
        let user_data = self.user_data.as_ref();
        writer.write_all(user_data).map_err(MessageError::io)?;
        Ok(4 + user_data.len())
    }
}
