use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::logical_address::LogicalAddress;

use super::message_error::MessageError;

/// Alive Check Response can be sent even if there was no Alive Check Request received.
/// It can also be sent by any `DoIP` entity to indicate that it is alive, server or client.
///
/// Typical use is to wait for a request from the server
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AliveCheckResponse {
    /// Contains the logical address of the client `DoIP` entity that is currently active on this `TCP_DATA` socket.
    pub source_address: LogicalAddress,
}

impl AliveCheckResponse {
    /// Deserialize an alive check response from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let source_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
        Ok(AliveCheckResponse { source_address })
    }

    /// Serialize this alive check response to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.source_address.into())?;
        Ok(2)
    }
}
