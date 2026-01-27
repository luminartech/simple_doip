//! Represents the header of a `DoIP` message
//!
//! Will check the protocol version and inverse protocol version to ensure
//! the message is valid.
use std::{
    fmt::Debug,
    io::{Read, Write},
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use super::message_error::MessageError;

/// `DoIP` Protocol Version
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
#[repr(u8)]
pub enum ProtocolVersion {
    Reserved = 0x00,
    /// ISO 13400-2:2010
    V2010 = 0x01,
    /// ISO 13400-2:2012
    V2012 = 0x02,
    /// ISO 13400-2:2019
    V2019 = 0x03,
    /// Client Future Spec Reserved
    ReservedFuture(u8),
    /// Client Version Value for Vehicle Identification Request
    VehicleIdentificationRequest = 0xFF,
}

impl ProtocolVersion {
    /// Returns the expected inverse value of the protocol version
    /// for verification of the message
    #[must_use]
    pub fn inverse(&self) -> u8 {
        match self {
            ProtocolVersion::Reserved => 0xFF,
            ProtocolVersion::V2010 => 0xFE,
            ProtocolVersion::V2012 => 0xFD,
            ProtocolVersion::V2019 => 0xFC,
            ProtocolVersion::ReservedFuture(value) => !value,
            ProtocolVersion::VehicleIdentificationRequest => 0x00,
        }
    }
}

impl From<u8> for ProtocolVersion {
    fn from(value: u8) -> Self {
        match value {
            0x00 => ProtocolVersion::Reserved,
            0x01 => ProtocolVersion::V2010,
            0x02 => ProtocolVersion::V2012,
            0x03 => ProtocolVersion::V2019,
            0x04..=0xFE => ProtocolVersion::ReservedFuture(value),
            0xFF => ProtocolVersion::VehicleIdentificationRequest,
        }
    }
}

impl From<ProtocolVersion> for u8 {
    fn from(value: ProtocolVersion) -> Self {
        match value {
            ProtocolVersion::Reserved => 0x00,
            ProtocolVersion::V2010 => 0x01,
            ProtocolVersion::V2012 => 0x02,
            ProtocolVersion::V2019 => 0x03,
            ProtocolVersion::ReservedFuture(value) => value,
            ProtocolVersion::VehicleIdentificationRequest => 0xFF,
        }
    }
}

impl Debug for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val: u8 = (*self).into();
        let val = format!("{} ({:#04X})", *self, val);
        f.write_str(&val)
    }
}

/// `DoIP` Message Payload Type
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
#[repr(u16)]
pub enum PayloadType {
    /// Negative Acknowledge
    /// Ignore packets with multi- or broadcast address as source IP address
    /// One message per UDP datagram
    NegativeAcknowledge = 0x0000,
    /// Client Vehicle Identification Request
    VehicleIdentificationRequest = 0x0001,
    /// Client Vehicle Identification Request with Entity ID (EID)
    VehicleIdentificationRequestWithEID = 0x0002,
    /// Client Vehicle Identification Request with Vehicle Identification Number (VIN)
    VehicleIdentificationRequestWithVIN = 0x0003,
    /// Client Vehicle Announcement Message
    VehicleAnnouncement = 0x0004,
    /// Client Routing Activation Request Message
    RoutingActivationRequest = 0x0005,
    /// Client Routing Activation Response Message
    RoutingActivationResponse = 0x0006,
    /// Client Alive Check Request Message
    AliveCheckRequest = 0x0007,
    /// Client Alive Check Response Message
    AliveCheckResponse = 0x0008,
    /// Client Entity Status Request Message
    DoIPEntityStatusRequest = 0x4001,
    /// Client Entity Status Response Message
    DoIPEntityStatusResponse = 0x4002,
    /// Client Diagnostic Power Mode Info Request Message
    DiagnosticPowerModeInfoRequest = 0x4003,
    /// Client Diagnostic Power Mode Info Response Message
    DiagnosticPowerModeInfoResponse = 0x4004,
    /// Client Diagnostic Message
    DiagnosticMessage = 0x8001,
    /// Client Diagnostic Message Positive Acknowledge
    DiagnosticMessagePositiveAcknowledge = 0x8002,
    /// Client Diagnostic Message Negative Acknowledge
    DiagnosticMessageNegativeAcknowledge = 0x8003,
    /// Client Spec Reserved
    Reserved(u16),
    /// Client Spec Reserved for Vehicle Manufacturer
    ReservedVehicleManufacturer(u16),
}

impl From<u16> for PayloadType {
    fn from(value: u16) -> Self {
        match value {
            0x0000 => PayloadType::NegativeAcknowledge,
            0x0001 => PayloadType::VehicleIdentificationRequest,
            0x0002 => PayloadType::VehicleIdentificationRequestWithEID,
            0x0003 => PayloadType::VehicleIdentificationRequestWithVIN,
            0x0004 => PayloadType::VehicleAnnouncement,
            0x0005 => PayloadType::RoutingActivationRequest,
            0x0006 => PayloadType::RoutingActivationResponse,
            0x0007 => PayloadType::AliveCheckRequest,
            0x0008 => PayloadType::AliveCheckResponse,
            0x4001 => PayloadType::DoIPEntityStatusRequest,
            0x4002 => PayloadType::DoIPEntityStatusResponse,
            0x4003 => PayloadType::DiagnosticPowerModeInfoRequest,
            0x4004 => PayloadType::DiagnosticPowerModeInfoResponse,
            0x8001 => PayloadType::DiagnosticMessage,
            0x8002 => PayloadType::DiagnosticMessagePositiveAcknowledge,
            0x8003 => PayloadType::DiagnosticMessageNegativeAcknowledge,
            0xF000..=0xFFFF => PayloadType::ReservedVehicleManufacturer(value),
            _ => PayloadType::Reserved(value),
        }
    }
}

impl From<PayloadType> for u16 {
    fn from(value: PayloadType) -> Self {
        match value {
            PayloadType::NegativeAcknowledge => 0x0000,
            PayloadType::VehicleIdentificationRequest => 0x0001,
            PayloadType::VehicleIdentificationRequestWithEID => 0x0002,
            PayloadType::VehicleIdentificationRequestWithVIN => 0x0003,
            PayloadType::VehicleAnnouncement => 0x0004,
            PayloadType::RoutingActivationRequest => 0x0005,
            PayloadType::RoutingActivationResponse => 0x0006,
            PayloadType::AliveCheckRequest => 0x0007,
            PayloadType::AliveCheckResponse => 0x0008,
            PayloadType::DoIPEntityStatusRequest => 0x4001,
            PayloadType::DoIPEntityStatusResponse => 0x4002,
            PayloadType::DiagnosticPowerModeInfoRequest => 0x4003,
            PayloadType::DiagnosticPowerModeInfoResponse => 0x4004,
            PayloadType::DiagnosticMessage => 0x8001,
            PayloadType::DiagnosticMessagePositiveAcknowledge => 0x8002,
            PayloadType::DiagnosticMessageNegativeAcknowledge => 0x8003,
            PayloadType::Reserved(value) | PayloadType::ReservedVehicleManufacturer(value) => value,
        }
    }
}

impl Debug for PayloadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val: u16 = (*self).into();
        let val = format!("{} ({:#04X})", *self, val);
        f.write_str(&val)
    }
}

/// `DoIP` Message Header
///
/// The header is 8 bytes long and contains the following fields:
/// * [`ProtocolVersion`] (1 byte)
/// * Inverse Protocol Version (1 byte)
/// * [`PayloadType`] (2 bytes)
/// * Payload Length (4 bytes)
#[derive(Clone, PartialEq)]
pub struct Header {
    /// Client Protocol Version
    pub protocol_version: ProtocolVersion,
    /// Bitwise inverse of `protocol_version` for verification
    pub inverse_protocol_version: u8,
    /// Client Payload Type
    pub payload_type: PayloadType,
    /// Length of payload byte array, does not include header.
    pub payload_length: u32,
}

impl Header {
    /// Create a new `DoIP` message header
    #[must_use]
    pub fn new(
        protocol_version: ProtocolVersion,
        payload_type: PayloadType,
        payload_length: u32,
    ) -> Self {
        let inverse_protocol_version = protocol_version.inverse();
        Header {
            protocol_version,
            inverse_protocol_version,
            payload_type,
            payload_length,
        }
    }
    /// Checks that the inverse value of the protocol version is correct
    /// to ensure a properly formatted DOIP message is received
    pub(crate) fn version_inverse_correct(&self) -> Result<(), MessageError> {
        let expected = self.protocol_version.inverse();
        if expected == self.inverse_protocol_version {
            Ok(())
        } else {
            Err(MessageError::VersionInverseIncorrect {
                expected,
                value: self.inverse_protocol_version,
            })
        }
    }
    /// Deserialize a `DoIP` header from a byte stream
    pub(crate) fn read<T: Read>(reader: &mut T) -> Result<Header, MessageError> {
        let protocol_version = reader.read_u8()?.into();
        let inverse_protocol_version = reader.read_u8()?;
        let payload_type = reader.read_u16::<BigEndian>()?.into();
        let payload_length = reader.read_u32::<BigEndian>()?;
        let header = Header {
            protocol_version,
            inverse_protocol_version,
            payload_type,
            payload_length,
        };
        header.version_inverse_correct()?;
        Ok(header)
    }
    /// Serialize this header to a byte stream
    pub(crate) fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u8(self.protocol_version.into())?;
        writer.write_u8(self.inverse_protocol_version)?;
        let payload_type: u16 = self.payload_type.into();
        writer.write_u16::<BigEndian>(payload_type)?;
        writer.write_u32::<BigEndian>(self.payload_length)?;
        Ok(8)
    }
}

impl Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("Version", &self.protocol_version)
            .field(
                "inverse_protocol_version",
                &format_args!("{:#X}", self.inverse_protocol_version),
            )
            .field("payload_type", &self.payload_type)
            .field("payload_length", &self.payload_length)
            .finish()
    }
}
