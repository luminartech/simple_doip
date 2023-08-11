use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// DoIP Protocol Version
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolVersion {
    Reserved,
    /// ISO 13400-2:2010
    V2010,
    /// ISO 13400-2:2012
    V2012,
    /// ISO 13400-2:2019
    V2019,
    /// DoIP Future Spec Reserved
    ReservedFuture(u8),
    /// DoiP Version Value for Vehicle Identification Request
    VehicleIdentificationRequest,
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

/// DoIP Message Payload Type
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadType {
    /// DoIP Negative Acknowledge
    /// Ignore packets with multi- or broadcast address as source IP address
    /// One DoIP message per UDP datagram
    NegativeAcknowledge,
    /// DoIP Vehicle Identification Request
    VehicleIdentificationRequest,
    /// DoIP Vehicle Identification Request with Entity ID (EID)
    VehicleIdentificationRequestWithEID,
    /// DoIP Vehicle Identification Request with Vehicle Identification Number (VIN)
    VehicleIdentificationRequestWithVIN,
    /// DoIP Vehicle Announcement Message
    VehicleAnnouncement,
    /// DoIP Routing Activation Request Message
    RoutingActivationRequest,
    /// DoIP Routing Activation Response Message
    RoutingActivationResponse,
    /// DoIP Alive Check Request Message
    AliveCheckRequest,
    /// DoIP Alive Check Response Message
    AliveCheckResponse,
    /// DoIP Entity Status Request Message
    DoIPEntityStatusRequest,
    /// DoIP Entity Status Response Message
    DoIPEntityStatusResponse,
    /// DoIP Diagnostic Power Mode Info Request Message
    DiagnosticPowerModeInfoRequest,
    /// DoIP Diagnostic Power Mode Info Response Message
    DiagnosticPowerModeInfoResponse,
    /// DoIP Diagnostic Message
    DiagnosticMessage,
    /// DoIP Diagnostic Message Positive Acknowledge
    DiagnosticMessagePositiveAcknowledge,
    /// DoIP Diagnostic Message Negative Acknowledge
    DiagnosticMessageNegativeAcknowledge,
    /// DoIP Spec Reserved
    Reserved(u16),
    /// DoIP Spec Reserved for Vehicle Manufacturer
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
            0x0009..=0x4000 => PayloadType::Reserved(value),
            0x4001 => PayloadType::DoIPEntityStatusRequest,
            0x4002 => PayloadType::DoIPEntityStatusResponse,
            0x4003 => PayloadType::DiagnosticPowerModeInfoRequest,
            0x4004 => PayloadType::DiagnosticPowerModeInfoResponse,
            0x4005..=0x8000 => PayloadType::Reserved(value),
            0x8001 => PayloadType::DiagnosticMessage,
            0x8002 => PayloadType::DiagnosticMessagePositiveAcknowledge,
            0x8003 => PayloadType::DiagnosticMessageNegativeAcknowledge,
            0x8004..=0xEFFF => PayloadType::Reserved(value),
            0xF000..=0xFFFF => PayloadType::ReservedVehicleManufacturer(value),
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
            PayloadType::Reserved(value) => value,
            PayloadType::DoIPEntityStatusRequest => 0x4001,
            PayloadType::DoIPEntityStatusResponse => 0x4002,
            PayloadType::DiagnosticPowerModeInfoRequest => 0x4003,
            PayloadType::DiagnosticPowerModeInfoResponse => 0x4004,
            PayloadType::DiagnosticMessage => 0x8001,
            PayloadType::DiagnosticMessagePositiveAcknowledge => 0x8002,
            PayloadType::DiagnosticMessageNegativeAcknowledge => 0x8003,
            PayloadType::ReservedVehicleManufacturer(value) => value,
        }
    }
}

/// DoIP Message Header
pub struct DoIpHeader {
    /// DoIP Protocol Version
    pub protocol_version: ProtocolVersion,
    /// Bitwise inverse of protocol_version for verification
    pub inverse_protocol_version: u8,
    /// DoIP Payload Type
    pub payload_type: PayloadType,
    /// Length of payload byte array, does not include header.
    pub payload_length: u32,
}

/// DoIP Message Header
impl DoIpHeader {
    pub(crate) fn version_inverse_correct(&self) -> bool {
        let protocol_version: u8 = self.protocol_version.into();
        protocol_version ^ 0xFF == self.inverse_protocol_version
    }
    pub fn read<T: Read>(reader: &mut T) -> DoIpHeader {
        let protocol_version = reader.read_u8().unwrap().into();
        println!("protocol_version: {:?}", protocol_version);
        let inverse_protocol_version = reader.read_u8().unwrap();
        let payload_type = reader.read_u16::<BigEndian>().unwrap().into();
        let payload_length = reader.read_u32::<BigEndian>().unwrap();
        DoIpHeader {
            protocol_version,
            inverse_protocol_version,
            payload_type,
            payload_length,
        }
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_u8(self.protocol_version.into()).unwrap();
        writer.write_u8(self.inverse_protocol_version).unwrap();
        let payload_type: u16 = self.payload_type.into();
        writer.write_u16::<BigEndian>(payload_type).unwrap();
        writer.write_u32::<BigEndian>(self.payload_length).unwrap();
    }
}
