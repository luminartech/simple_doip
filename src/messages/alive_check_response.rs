use std::io::{Read, Write};

use crate::error::DoIPError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliveCheckResponse {
    /// Contains the logical address of the client DoIP entity that is currently active on this TCP_DATA socket.
    pub source_address: u16,
}

impl AliveCheckResponse {
    pub(crate) fn read<T: Read>(reader: &mut T) -> Result<Self, DoIPError> {
        let mut source_address = [0x00u8; 2];
        reader.read_exact(&mut source_address)?;
        Ok(AliveCheckResponse {
            source_address: u16::from_be_bytes(source_address),
        })
    }

    pub(crate) fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPError> {
        writer.write_all(&self.source_address.to_be_bytes())?;
        Ok(())
    }
}
