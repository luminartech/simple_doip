use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use super::message_error::DoIPMessageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityStatusNodeType {
    DoIPGateway,
    DoIPNode,
    Reserved(u8),
}

impl From<u8> for EntityStatusNodeType {
    fn from(value: u8) -> Self {
        match value {
            0x00 => EntityStatusNodeType::DoIPGateway,
            0x01 => EntityStatusNodeType::DoIPNode,
            0x02..=0xFF => EntityStatusNodeType::Reserved(value),
        }
    }
}
impl From<EntityStatusNodeType> for u8 {
    fn from(value: EntityStatusNodeType) -> Self {
        match value {
            EntityStatusNodeType::DoIPGateway => 0x00,
            EntityStatusNodeType::DoIPNode => 0x01,
            EntityStatusNodeType::Reserved(value) => value,
        }
    }
}

///This payload type serves the purpose of identifying certain operating conditions of the responding DoIP entity.
/// This allows, for example a client DoIP entity to detect existing diagnostic communication sessions as well as the capabilities of a DoIP entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityStatusResponse {
    node_type: EntityStatusNodeType,
    max_concurrent_tcp_sockets: u8,
    open_tcp_sockets: u8,
    max_data_size: u32,
}

impl EntityStatusResponse {
    pub(crate) fn read<T: Read>(reader: &mut T) -> Result<Self, DoIPMessageError> {
        let node_type = EntityStatusNodeType::from(reader.read_u8()?);
        let max_concurrent_tcp_sockets = reader.read_u8()?;
        let open_tcp_sockets = reader.read_u8()?;
        let max_data_size = reader.read_u32::<BigEndian>()?;
        Ok(EntityStatusResponse {
            node_type,
            max_concurrent_tcp_sockets,
            open_tcp_sockets,
            max_data_size,
        })
    }

    pub(crate) fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        writer.write_u8(self.node_type.into())?;
        writer.write_u8(self.max_concurrent_tcp_sockets)?;
        writer.write_u8(self.open_tcp_sockets)?;
        writer.write_u32::<BigEndian>(self.max_data_size)?;
        Ok(7)
    }
}
