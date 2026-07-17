use crate::LogicalAddress;

use automotive_wire_codec::{read_u16_be, write_all, write_u16_be};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticMessage<D> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: D,
}

impl<'a> Decode<'a> for DiagnosticMessage<&'a [u8]> {
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

impl<D: AsRef<[u8]>> Encode for DiagnosticMessage<D> {
    type Error = MessageError;

    /// Serialize this diagnostic message into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        write_u16_be(writer, self.target_address.into())?;
        let user_data = self.user_data.as_ref();
        write_all(writer, user_data)?;
        Ok(4 + user_data.len())
    }
}
