use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::LogicalAddress;

use super::message_error::MessageError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticMessage {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: Vec<u8>,
}

impl DiagnosticMessage {
    /// Deserialize a diagnostic message from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, MessageError> {
        let source_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
        let target_address = LogicalAddress(reader.read_u16::<BigEndian>()?);

        // Read remaining bytes as opaque user data
        let mut user_data = Vec::new();
        reader.read_to_end(&mut user_data)?;

        Ok(Self {
            source_address,
            target_address,
            user_data,
        })
    }

    /// Serialize this diagnostic message to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.source_address.into())?;
        writer.write_u16::<BigEndian>(self.target_address.into())?;
        writer.write_all(&self.user_data)?;
        Ok(4 + self.user_data.len())
    }
}
