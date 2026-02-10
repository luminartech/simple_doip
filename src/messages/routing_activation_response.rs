use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::logical_address::LogicalAddress;

use super::message_error::MessageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum RoutingActivationResponseCode {
    /// Routing activation denied due to unknown source address.
    ///  * Do not activate routing and close this `TCP_DATA` socket.
    DeniedUnknownSourceAddress = 0x00,
    /// Routing activation denied because all concurrently supported `TCP_DATA` sockets are registered and active.
    /// * Do not activate routing and close this `TCP_DATA` socket.
    DeniedAllTcpSocketsRegisteredAndActive = 0x01,
    /// Routing activation denied because the SA received is different from the table connection entry on the already activated `TCP_DATA` socket.
    /// * Do not activate routing and close this `TCP_DATA` socket.
    DeniedSourceAddressAlreadyActivated = 0x02,
    /// Routing activation denied because the SA is already registered and active on a different `TCP_DATA` socket.
    /// * Do not activate routing and close this `TCP_DATA` socket.
    DeniedSourceAddressAlreadyRegistered = 0x03,
    /// Routing activation denied due to missing authentication.
    /// * Do not activate routing and register.
    DeniedMissingAuthentication = 0x04,
    /// Routing activation denied due to rejected confirmation.
    /// * Do not activate routing and close this `TCP_DATA` socket.
    DeniedRejectedConfirmation = 0x05,
    /// Routing activation denied due to unsupported routing activation type.
    /// * Do not activate routing and close this `TCP_DATA` socket.
    DeniedUnsupportedRoutingActivationType = 0x06,
    /// Routing activation denied because the specified activation type requires a secure TLS `TCP_DATA` socket.
    /// * Do not activate routing and close this (non TLS) `TCP_DATA` socket.
    DeniedEncryptedConnectionViaTLSRequired = 0x07,
    /// Reserved for future use.
    /// * Ignored by this library.
    Reserved(u8),
    /// Routing successfully activated.
    /// * Activate routing and register SA on this `TCP_DATA` socket.
    RoutingSuccessfullyActivated = 0x10,
    /// Routing is activated; confirmation required.
    /// * Only activate routing after confirmation from within the vehicle.
    RoutingSuccessfullyActivatedConfirmationRequired = 0x11,
    /// Vehicle manufacturer specific response code.
    /// * Ignored by this library.
    VehicleManufacturerSpecific(u8),
}

impl From<u8> for RoutingActivationResponseCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => RoutingActivationResponseCode::DeniedUnknownSourceAddress,
            0x01 => RoutingActivationResponseCode::DeniedAllTcpSocketsRegisteredAndActive,
            0x02 => RoutingActivationResponseCode::DeniedSourceAddressAlreadyActivated,
            0x03 => RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered,
            0x04 => RoutingActivationResponseCode::DeniedMissingAuthentication,
            0x05 => RoutingActivationResponseCode::DeniedRejectedConfirmation,
            0x06 => RoutingActivationResponseCode::DeniedUnsupportedRoutingActivationType,
            0x07 => RoutingActivationResponseCode::DeniedEncryptedConnectionViaTLSRequired,
            0x10 => RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            0x11 => RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired,
            0xE0..=0xFE => RoutingActivationResponseCode::VehicleManufacturerSpecific(value),
            _ => RoutingActivationResponseCode::Reserved(value),
        }
    }
}

impl From<RoutingActivationResponseCode> for u8 {
    fn from(value: RoutingActivationResponseCode) -> Self {
        match value {
            RoutingActivationResponseCode::DeniedUnknownSourceAddress => 0x00,
            RoutingActivationResponseCode::DeniedAllTcpSocketsRegisteredAndActive => 0x01,
            RoutingActivationResponseCode::DeniedSourceAddressAlreadyActivated => 0x02,
            RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered => 0x03,
            RoutingActivationResponseCode::DeniedMissingAuthentication => 0x04,
            RoutingActivationResponseCode::DeniedRejectedConfirmation => 0x05,
            RoutingActivationResponseCode::DeniedUnsupportedRoutingActivationType => 0x06,
            RoutingActivationResponseCode::DeniedEncryptedConnectionViaTLSRequired => 0x07,
            RoutingActivationResponseCode::RoutingSuccessfullyActivated => 0x10,
            RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired => 0x11,
            RoutingActivationResponseCode::Reserved(value)
            | RoutingActivationResponseCode::VehicleManufacturerSpecific(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingActivationResponse {
    /// External test equipment address
    pub logical_address_tester: LogicalAddress,
    /// Routing activation status information
    pub logical_address_of_doip_entity: LogicalAddress,
    pub routing_activation_response_code: RoutingActivationResponseCode,
    pub reserved_oem: [u8; 4],
    pub oem_specific: Option<[u8; 4]>,
}

impl RoutingActivationResponse {
    /// Deserialize a routing activation response from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let logical_address_tester = LogicalAddress(reader.read_u16::<BigEndian>()?);
        let logical_address_of_doip_entity = LogicalAddress(reader.read_u16::<BigEndian>()?);
        let routing_activation_response_code_byte: u8 = reader.read_u8()?;
        let routing_activation_response_code =
            RoutingActivationResponseCode::from(routing_activation_response_code_byte);

        let mut reserved_oem = [0x00u8; 4];
        reader.read_exact(&mut reserved_oem)?;
        let mut oem_specific = [0x00u8; 4];
        let oem_specific = match reader.read_exact(&mut oem_specific) {
            Ok(()) => Some(oem_specific),
            Err(_) => None,
        };

        Ok(RoutingActivationResponse {
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        })
    }

    /// Serialize this routing activation response to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.logical_address_tester.into())?;
        writer.write_u16::<BigEndian>(self.logical_address_of_doip_entity.into())?;
        writer.write_u8(self.routing_activation_response_code.into())?;
        writer.write_all(&self.reserved_oem)?;
        if let Some(oem_specific) = self.oem_specific {
            writer.write_all(&oem_specific)?;
        }
        Ok(9 + self.oem_specific.map_or(0, |_| 4))
    }
}
