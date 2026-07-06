use crate::logical_address::LogicalAddress;

use super::decode_util::read_u16_be;
use super::encode_util::write_u16_be;
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// Alive Check Response can be sent even if there was no Alive Check Request received.
/// It can also be sent by any `DoIP` entity to indicate that it is alive, server or client.
///
/// Typical use is to wait for a request from the server
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AliveCheckResponse {
    /// Contains the logical address of the client `DoIP` entity that is currently active on this `TCP_DATA` socket.
    pub source_address: LogicalAddress,
}

impl<'a> Decode<'a> for AliveCheckResponse {
    /// Deserialize an alive check response from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        Ok((
            AliveCheckResponse {
                source_address: LogicalAddress(source_address),
            },
            rest,
        ))
    }
}

impl Encode for AliveCheckResponse {
    fn encoded_size(&self) -> usize {
        2
    }

    /// Serialize this alive check response into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        Ok(2)
    }
}
