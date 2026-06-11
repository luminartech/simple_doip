use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use super::message_error::MessageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum EntityStatusNodeType {
    DoIPGateway = 0x00,
    DoIPNode = 0x01,
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

/// This payload type serves the purpose of identifying certain operating
/// conditions of the responding `DoIP` entity.
/// This allows, for example a client `DoIP` entity to detect existing diagnostic
/// communication sessions as well as the capabilities of a `DoIP` entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityStatusResponse {
    pub node_type: EntityStatusNodeType,
    pub max_concurrent_tcp_sockets: u8,
    pub open_tcp_sockets: u8,
    pub max_data_size: u32,
}

impl EntityStatusResponse {
    /// Deserialize an entity status response from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
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

    /// Serialize this entity status response to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u8(self.node_type.into())?;
        writer.write_u8(self.max_concurrent_tcp_sockets)?;
        writer.write_u8(self.open_tcp_sockets)?;
        writer.write_u32::<BigEndian>(self.max_data_size)?;
        Ok(7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_entity_node_type_roundtrip(byte in any::<u8>()) {
            let code = EntityStatusNodeType::from(byte);
            let back: u8 = code.into();
            prop_assert_eq!(byte, back);
        }

        #[test]
        fn prop_entity_status_response_roundtrip(
            node_byte in any::<u8>(),
            max_tcp in any::<u8>(),
            open_tcp in any::<u8>(),
            max_data in any::<u32>(),
        ) {
            let resp = EntityStatusResponse {
                node_type: EntityStatusNodeType::from(node_byte),
                max_concurrent_tcp_sockets: max_tcp,
                open_tcp_sockets: open_tcp,
                max_data_size: max_data,
            };
            let mut buf = Vec::new();
            resp.write(&mut buf).unwrap();

            let parsed = EntityStatusResponse::read(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(resp, parsed);
        }
    }
}
