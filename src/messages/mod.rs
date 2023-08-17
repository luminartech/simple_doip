pub mod alive_check_response;
pub mod diagnostic_message;
pub mod diagnostic_message_ack;
pub mod entity_status_response;
pub mod header;
pub mod nack;
pub mod power_mode_info_response;
pub mod routing_activation_request;
pub mod routing_activation_response;
pub mod vehicle_identification_response;

use crate::{
    error::DoIPError,
    messages::{
        alive_check_response::AliveCheckResponse, diagnostic_message::DiagnosticMessage,
        entity_status_response::EntityStatusResponse, header::DoIpHeader, nack::NackCode,
        power_mode_info_response::DiagnosticPowerModeCode,
        routing_activation_request::RoutingActivationRequest,
        routing_activation_response::RoutingActivationResponse,
        vehicle_identification_response::VehicleIdentificationResponse,
    },
};

use self::diagnostic_message_ack::DiagnosticMessageAck;

/// DoIP Message Payload Type
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoIPPayload {
    /// DoIP Negative Acknowledge
    /// Ignore packets with multi- or broadcast address as source IP address
    /// One DoIP message per UDP datagram
    NegativeAcknowledge(NackCode),
    /// DoIP Vehicle Identification Request
    VehicleIdentificationRequest,
    /// DoIP Vehicle Identification Request with Entity ID (EID)
    VehicleIdentificationRequestWithEID([u8; 6]),
    /// DoIP Vehicle Identification Request with Vehicle Identification Number (VIN)
    VehicleIdentificationRequestWithVIN([u8; 17]),
    /// DoIP Vehicle Announcement Message
    VehicleAnnouncement(VehicleIdentificationResponse),
    /// DoIP Routing Activation Request Message
    RoutingActivationRequest(RoutingActivationRequest),
    /// DoIP Routing Activation Response Message
    RoutingActivationResponse(RoutingActivationResponse),
    /// DoIP Alive Check Request Message
    AliveCheckRequest,
    /// DoIP Alive Check Response Message
    AliveCheckResponse(AliveCheckResponse),
    /// DoIP Entity Status Request Message
    DoIPEntityStatusRequest,
    /// DoIP Entity Status Response Message
    DoIPEntityStatusResponse(EntityStatusResponse),
    /// DoIP Diagnostic Power Mode Info Request Message
    DiagnosticPowerModeInfoRequest,
    /// DoIP Diagnostic Power Mode Info Response Message
    DiagnosticPowerModeInfoResponse(DiagnosticPowerModeCode),
    /// DoIP Diagnostic Message
    DiagnosticMessage(DiagnosticMessage),
    /// DoIP Diagnostic Message Positive Acknowledge
    DiagnosticMessagePositiveAcknowledge(DiagnosticMessageAck),
    /// DoIP Spec Reserved
    Reserved(u16),
    /// DoIP Spec Reserved for Vehicle Manufacturer
    ReservedVehicleManufacturer(u16),
}
pub struct DoIPMessage {
    pub header: header::DoIpHeader,
    pub payload: DoIPPayload,
}

pub struct DoIPParser {}

impl DoIPParser {
    pub fn parse_doip_message(message_bytes: &mut [u8]) -> Result<DoIPMessage, DoIPError> {
        let header = DoIpHeader::read(&mut message_bytes.as_ref());
        header.version_inverse_correct()?;
        let payload = message_bytes[8..].to_vec();
        if header.payload_length != payload.len() as u32 {
            return Err(DoIPError::PayloadLengthIncorrect {
                value: payload.len(),
                expected: header.payload_length,
            });
        }
        Ok(DoIPMessage {
            header,
            payload: DoIPPayload::NegativeAcknowledge(NackCode::from(0)),
        })
    }
}

trait DoIPMessagePayload {
    fn encoded_size(&self) -> usize;
    fn read<T: std::io::Read>(reader: &mut T, payload_length: u32) -> Result<Self, DoIPError>
    where
        Self: Sized;
    fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), DoIPError>;
}
#[cfg(test)]
mod tests {

    use super::*;
    use header::{PayloadType, ProtocolVersion};

    /// Check that we properly decode and encode hex bytes
    #[test]
    fn test_valid_messages() {
        let mut buf: Vec<u8> = vec![
            0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let deserialized_message = DoIPParser::parse_doip_message(&mut buf).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 8);
        buf = vec![
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message = DoIPParser::parse_doip_message(&mut buf).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2010);
        assert!(
            deserialized_message.header.payload_type == PayloadType::VehicleIdentificationRequest
        );
        assert!(deserialized_message.header.payload_length == 7);
    }
    #[test]
    fn test_invalid_inverse() {
        let mut buf: Vec<u8> = vec![0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // This parsing should panic for the bad inverse
        // TODO: Instead of panicking need to add error handling and return a result
        assert!(DoIPParser::parse_doip_message(&mut buf).is_err());
    }
}
