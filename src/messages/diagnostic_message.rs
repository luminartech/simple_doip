use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use super::message_error::DoIPMessageError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessage {
    pub source_address: u16,
    pub target_address: u16,
    pub user_data: Vec<u8>,
}

impl DiagnosticMessage {
    pub fn read<T: Read>(reader: &mut T, payload_length: usize) -> Result<Self, DoIPMessageError> {
        let source_address = reader.read_u16::<BigEndian>()?;

        let target_address = reader.read_u16::<BigEndian>()?;

        let user_data_len = payload_length - 4; // 4 == source + target address
        let mut user_data = Vec::with_capacity(user_data_len as usize);
        if user_data_len > 0 {
            reader.read_exact(&mut user_data)?;
        }

        Ok(Self {
            source_address,
            target_address,
            user_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPMessageError> {
        writer.write_u16::<BigEndian>(self.source_address)?;
        writer.write_u16::<BigEndian>(self.target_address)?;
        writer.write_all(&self.user_data)?;
        Ok(())
    }
}
