use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use uds_protocol::SingleValueWireFormat;

use super::message_error::DoIPMessageError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessage<DiagnosticsDefinition> {
    pub source_address: u16,
    pub target_address: u16,
    pub user_data: DiagnosticsDefinition,
}

impl<DiagnosticsDefinition: SingleValueWireFormat> DiagnosticMessage<DiagnosticsDefinition> {
    pub fn read<R: Read>(reader: &mut R, payload_length: usize) -> Result<Self, DoIPMessageError> {
        let source_address = reader.read_u16::<BigEndian>()?;
        let target_address = reader.read_u16::<BigEndian>()?;

        let user_data_len = payload_length - 4; // 4 == source + target address
        let mut user_data = vec![0; user_data_len];
        reader.read_exact(&mut user_data)?;
        let user_data = DiagnosticsDefinition::from_reader(&mut user_data.as_slice())?;

        Ok(Self {
            source_address,
            target_address,
            user_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        writer.write_u16::<BigEndian>(self.source_address)?;
        writer.write_u16::<BigEndian>(self.target_address)?;
        Ok(4 + self.user_data.to_writer(writer)?)
    }
}
