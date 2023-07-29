use std::mem::transmute;

use bincode::deserialize;
use serde::Deserialize;

/// DoIP Protocol Version
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[repr(u8)]
pub enum ProtocolVersion {
    /// ISO 13400-2:2010
    V2010 = 0x01,
    /// ISO 13400-2:2012
    V2012 = 0x02,
    /// ISO 13400-2:2019
    V2019 = 0x03,
}

/// DoIP Message Payload Type
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[repr(u16)]
pub enum PayloadType {
    /// DoIP Negative Acknowledge
    /// Ignore packets with multi- or broadcast address as source IP address
    /// One DoIP message per UDP datagram
    NegativeAcknowledge = 0x0000,
    /// DoIP Vehicle Identification Request
    VehicleIdentificationRequest = 0x0001,
    /// DoIP Vehicle Identification Request with Entity ID (EID)
    VehicleIdentificationRequestWithEID = 0x0002,
    /// DoIP Vehicle Identification Request with Vehicle Identification Number (VIN)
    VehicleIdentificationRequestWithVIN = 0x0003,
    VehicleAnnouncement = 0x0004,
    RoutingActivationRequest = 0x0005,
    RoutingActivationResponse = 0x0006,
    AliveCheckRequest = 0x0007,
    AliveCheckResponse = 0x0008,
    DoIPEntityStatusRequest = 0x4001,
    DoIPEntityStatusResponse = 0x4002,
    DiagnosticPowerModeInfoRequest = 0x4003,
    DiagnosticPowerModeInfoResponse = 0x4004,
    DiagnosticMessage = 0x8001,
    DiagnosticMessagePositiveAcknowledge = 0x8002,
    DiagnosticMessageNegativeAcknowledge = 0x8003,
}

/// DoIP Message Header
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[repr(C, packed)]
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
    fn version_inverse_correct(&self) -> bool {
        self.protocol_version as u8 ^ 0xFF == self.inverse_protocol_version
    }
}

#[derive(Clone, Debug, PartialEq)]
#[repr(C, packed)]
pub struct DoIPMessage {
    pub header: DoIpHeader,
    pub payload: [u8],
}

impl DoIPMessage {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 8 {
            // TODO return error
            panic!("Invalid DoIP message length");
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Check that we properly decode and encode hex bytes
    #[test]
    fn test_valid_messages() {
        let empty: [u8; 8] = [0x00, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let deserialized_message = DoIPMessage::from_bytes(&empty);
        assert!(deserialized_message.header.version_inverse_correct());
    }
}
