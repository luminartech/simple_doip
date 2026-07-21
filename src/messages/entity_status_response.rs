use automotive_wire_codec::{read_u8, read_u32_be, write_u8, write_u32_be};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// Classifies the kind of `DoIP` node reporting its status, per ISO 13400-2 §7.4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum EntityStatusNodeType {
    /// A `DoIP` gateway: bridges the IP network to one or more in-vehicle
    /// diagnostic networks, and may have multiple concurrent TCP sockets.
    DoIPGateway = 0x00,
    /// A `DoIP` node: a single ECU directly reachable over IP, supporting exactly
    /// one diagnostic TCP socket.
    DoIPNode = 0x01,
    /// A node type value outside the range this crate models.
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
    /// Whether the responding entity is a gateway or a single node.
    pub node_type: EntityStatusNodeType,
    /// The maximum number of concurrent diagnostic TCP sockets this entity supports.
    pub max_concurrent_tcp_sockets: u8,
    /// The number of diagnostic TCP sockets currently open on this entity.
    pub open_tcp_sockets: u8,
    /// The maximum diagnostic message payload size, in bytes, this entity can accept.
    pub max_data_size: u32,
}

impl<'a> Decode<'a> for EntityStatusResponse {
    type Error = MessageError;

    /// Deserialize an entity status response from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
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
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(7)
    }

    /// Serialize this entity status response into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u8(writer, self.node_type.into())?;
        write_u8(writer, self.max_concurrent_tcp_sockets)?;
        write_u8(writer, self.open_tcp_sockets)?;
        write_u32_be(writer, self.max_data_size)?;
        Ok(7)
    }
}
