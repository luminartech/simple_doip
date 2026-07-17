use crate::logical_address::LogicalAddress;

use automotive_wire_codec::{
    read_array, read_optional_array, read_u8, read_u16_be, write_all, write_u8, write_u16_be,
};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

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

impl<'a> Decode<'a> for RoutingActivationResponse {
    type Error = MessageError;

    /// Deserialize a routing activation response from a byte slice
    ///
    /// The optional OEM-specific tail is decoded when at least 4 bytes remain after the
    /// fixed fields.
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (logical_address_tester, rest) = read_u16_be(buf)?;
        let (logical_address_of_doip_entity, rest) = read_u16_be(rest)?;
        let (routing_activation_response_code, rest) = read_u8(rest)?;
        let routing_activation_response_code =
            RoutingActivationResponseCode::from(routing_activation_response_code);

        let (reserved_oem, rest) = read_array::<4>(rest)?;

        let (oem_specific, rest) = read_optional_array::<4>(rest);

        Ok((
            RoutingActivationResponse {
                logical_address_tester: LogicalAddress(logical_address_tester),
                logical_address_of_doip_entity: LogicalAddress(logical_address_of_doip_entity),
                routing_activation_response_code,
                reserved_oem,
                oem_specific,
            },
            rest,
        ))
    }
}

impl Encode for RoutingActivationResponse {
    type Error = MessageError;

    /// Serialize this routing activation response into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u16_be(writer, self.logical_address_tester.into())?;
        write_u16_be(writer, self.logical_address_of_doip_entity.into())?;
        write_u8(writer, self.routing_activation_response_code.into())?;
        write_all(writer, &self.reserved_oem)?;
        if let Some(oem_specific) = self.oem_specific {
            write_all(writer, &oem_specific)?;
        }
        Ok(9 + self.oem_specific.map_or(0, |_| 4))
    }
}
