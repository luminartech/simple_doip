use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use super::message_error::DoIPMessageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingActivationResponseCode {
    /// Routing activation denied due to unknown source address.
    ///  * Do not activate routing and close this TCP_DATA socket.
    DeniedUnknownSourceAddress,
    /// Routing activation denied because all concurrently supported TCP_DATA sockets are registered and active.
    /// * Do not activate routing and close this TCP_DATA socket.
    DeniedAllTcpSocketsRegisteredAndActive,
    /// Routing activation denied because the SA received is different from the table connection entry on the already activated TCP_DATA socket.
    /// * Do not activate routing and close this TCP_DATA socket.
    DeniedSourceAddressAlreadyActivated,
    /// Routing activation denied because the SA is already registered and active on a different TCP_DATA socket.
    /// * Do not activate routing and close this TCP_DATA socket.
    DeniedSourceAddressAlreadyRegistered,
    /// Routing activation denied due to missing authentication.
    /// * Do not activate routing and register.
    DeniedMissingAuthentication,
    /// Routing activation denied due to rejected confirmation.
    /// * Do not activate routing and close this TCP_DATA socket.
    DeniedRejectedConfirmation,
    /// Routing activation denied due to unsupported routing activation type.
    /// * Do not activate routing and close this TCP_DATA socket.
    DeniedUnsupportedRoutingActivationType,
    /// Routing activation denied because the specified activation type requires a secure TLS TCP_DATA socket.
    /// * Do not activate routing and close this (non TLS) TCP_DATA socket.
    DeniedEncryptedConnectionViaTLSRequired,
    /// Reserved for future use.
    /// * Ignored by this library.
    Reserved(u8),
    /// Routing successfully activated.
    /// * Activate routing and register SA on this TCP_DATA socket.
    RoutingSuccessfullyActivated,
    /// Routing is activated; confirmation required.
    /// * Only activate routing after confirmation from within the vehicle.
    RoutingSuccessfullyActivatedConfirmationRequired,
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
            0x08..=0x0F => RoutingActivationResponseCode::Reserved(value),
            0x10 => RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            0x11 => RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired,
            0x12..=0xDF => RoutingActivationResponseCode::Reserved(value),
            0xE0..=0xFE => RoutingActivationResponseCode::VehicleManufacturerSpecific(value),
            0xFF => RoutingActivationResponseCode::Reserved(value),
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
            RoutingActivationResponseCode::Reserved(value) => value,
            RoutingActivationResponseCode::RoutingSuccessfullyActivated => 0x10,
            RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired => 0x11,
            RoutingActivationResponseCode::VehicleManufacturerSpecific(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutingActivationResponse {
    /// External test equipment address
    pub logical_address_tester: u16,
    /// Routing activation status information
    pub logical_address_of_doip_entity: u16,
    pub routing_activation_response_code: RoutingActivationResponseCode,
    pub reserved_oem: [u8; 4],
    pub oem_specific: Option<[u8; 4]>,
}

impl RoutingActivationResponse {
    pub fn read<T: Read>(reader: &mut T, payload_length: usize) -> Result<Self, DoIPMessageError> {
        let logical_address_tester = reader.read_u16::<BigEndian>()?;
        let logical_address_of_doip_entity = reader.read_u16::<BigEndian>()?;
        let routing_activation_response_code_byte = reader.read_u8()?;
        let routing_activation_response_code =
            RoutingActivationResponseCode::from(routing_activation_response_code_byte);

        let mut reserved_oem = [0x00u8; 4];
        reader.read_exact(&mut reserved_oem)?;

        let oem_specific = if payload_length == 13 {
            let mut oem_specific = [0x00u8; 4];
            reader.read_exact(&mut oem_specific)?;
            Some(oem_specific)
        } else {
            None
        };
        Ok(RoutingActivationResponse {
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        writer.write_all(&self.logical_address_tester.to_be_bytes())?;
        writer.write_all(&self.logical_address_of_doip_entity.to_be_bytes())?;
        writer.write_u8(self.routing_activation_response_code.into())?;
        writer.write_all(&self.reserved_oem)?;
        if let Some(oem_specific) = self.oem_specific {
            writer.write_all(&oem_specific)?;
        }
        Ok(13 + self.oem_specific.map(|_| 4).unwrap_or(0))
    }
}
