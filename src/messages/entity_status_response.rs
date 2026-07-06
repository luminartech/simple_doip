use super::decode_util::{read_u8, read_u32_be};
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

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

impl<'a> Decode<'a> for EntityStatusResponse {
    /// Deserialize an entity status response from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (node_type, rest) = read_u8(buf)?;
        let node_type = EntityStatusNodeType::from(node_type);
        let (max_concurrent_tcp_sockets, rest) = read_u8(rest)?;
        let (open_tcp_sockets, rest) = read_u8(rest)?;
        let (max_data_size, rest) = read_u32_be(rest)?;
        Ok((
            EntityStatusResponse {
                node_type,
                max_concurrent_tcp_sockets,
                open_tcp_sockets,
                max_data_size,
            },
            rest,
        ))
    }
}

impl Encode for EntityStatusResponse {
    fn encoded_size(&self) -> usize {
        7
    }

    /// Serialize this entity status response into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        writer
            .write_all(&[self.node_type.into()])
            .map_err(MessageError::io)?;
        writer
            .write_all(&[self.max_concurrent_tcp_sockets])
            .map_err(MessageError::io)?;
        writer
            .write_all(&[self.open_tcp_sockets])
            .map_err(MessageError::io)?;
        writer
            .write_all(&u32::to_be_bytes(self.max_data_size))
            .map_err(MessageError::io)?;
        Ok(7)
    }
}
