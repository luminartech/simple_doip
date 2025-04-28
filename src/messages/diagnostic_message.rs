use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use uds_protocol::WireFormat;

use crate::LogicalAddress;

use super::message_error::MessageError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessage<DiagnosticsDefinition> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: DiagnosticsDefinition,
}

impl<DiagnosticsDefinition: WireFormat> DiagnosticMessage<DiagnosticsDefinition> {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, MessageError> {
        let source_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
        let target_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
        let user_data = DiagnosticsDefinition::option_from_reader(reader)?
            .expect("Diagnostics Messages should never be empty");

        Ok(Self {
            source_address,
            target_address,
            user_data,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.source_address.into())?;
        writer.write_u16::<BigEndian>(self.target_address.into())?;
        Ok(4 + self.user_data.to_writer(writer)?)
    }
}
