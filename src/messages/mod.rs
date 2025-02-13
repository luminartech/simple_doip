mod alive_check_response;
pub use alive_check_response::AliveCheckResponse;
mod diagnostic_message;
pub use diagnostic_message::DiagnosticMessage;
mod diagnostic_message_ack;
pub use diagnostic_message_ack::{DiagnosticAckCode, DiagnosticMessageAck};
mod entity_status_response;
pub use entity_status_response::{EntityStatusNodeType, EntityStatusResponse};
mod header;
pub use header::{DoIPHeader, PayloadType, ProtocolVersion};
mod message_error;
pub use message_error::DoIPMessageError;
mod nack;
pub use nack::NackCode;
mod payload;
pub use payload::Payload;
mod power_mode_info_response;
pub use power_mode_info_response::DiagnosticPowerModeCode;
mod routing_activation_request;
pub use routing_activation_request::{ActivationTypeCode, RoutingActivationRequest};
mod routing_activation_response;
pub use routing_activation_response::{RoutingActivationResponse, RoutingActivationResponseCode};
mod vehicle_identification_response;
use uds_protocol::SingleValueWireFormat;
pub use vehicle_identification_response::{
    FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
};

use std::io::{Read, Write};

#[derive(Debug)]
pub struct DoIPMessage<DiagnosticDefinitions> {
    pub header: header::DoIPHeader,
    pub payload: Payload<DiagnosticDefinitions>,
}

impl<DiagnosticsDefinition: SingleValueWireFormat> DoIPMessage<DiagnosticsDefinition> {
    pub fn alive_check_request(
        protocol_version: ProtocolVersion,
    ) -> DoIPMessage<DiagnosticsDefinition> {
        DoIPMessage {
            header: DoIPHeader::new(protocol_version, PayloadType::AliveCheckRequest, 0),
            payload: Payload::NoPayload,
        }
    }

    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: u16,
    ) -> DoIPMessage<DiagnosticsDefinition> {
        let response = AliveCheckResponse { source_address };
        DoIPMessage {
            header: DoIPHeader::new(protocol_version, PayloadType::AliveCheckResponse, 0),
            payload: Payload::AliveCheckResponse(response),
        }
    }

    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: u16,
        target_address: u16,
        message: DiagnosticsDefinition,
    ) -> DoIPMessage<DiagnosticsDefinition> {
        let message = DiagnosticMessage {
            source_address,
            target_address,
            user_data: message,
        };
        DoIPMessage {
            header: DoIPHeader::new(protocol_version, PayloadType::DiagnosticMessage, 0),
            payload: Payload::DiagnosticMessage(message),
        }
    }

    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: u16,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> DoIPMessage<DiagnosticsDefinition> {
        let request = RoutingActivationRequest {
            source_address,
            activation_type,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer,
        };

        let mut payload = Vec::with_capacity(11);
        request.write(&mut payload).unwrap();

        let header = DoIPHeader::new(
            protocol_version,
            PayloadType::RoutingActivationRequest,
            payload.len() as u32,
        );
        DoIPMessage {
            header,
            payload: Payload::RoutingActivationRequest(request),
        }
    }
}

impl<DiagnosticsDefinition: SingleValueWireFormat> DoIPMessage<DiagnosticsDefinition> {
    // TODO: This needs careful review and should do a lot more error checking than it does now
    pub fn read<T: Read>(
        mut message_bytes: &mut T,
    ) -> Result<DoIPMessage<DiagnosticsDefinition>, DoIPMessageError> {
        let header = DoIPHeader::read(&mut message_bytes)?;
        header.version_inverse_correct()?;
        let payload = match header.payload_type {
            PayloadType::AliveCheckResponse => {
                Payload::AliveCheckResponse(AliveCheckResponse::read(&mut message_bytes)?)
            }
            _ => return Err(DoIPMessageError::UnexpectedPayloadType(header.payload_type)),
        };
        Ok(DoIPMessage { header, payload })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        let mut written = self.header.write(writer)?;
        written += &self.payload.write(writer)?;
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use header::{PayloadType, ProtocolVersion};
    use uds_protocol::ProtocolIdentifier;

    /// Check that we properly decode and encode hex bytes
    #[test]
    fn test_valid_messages() {
        let buf: [u8; 16] = [
            0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let deserialized_message: DoIPMessage<uds_protocol::ProtocolRequest> =
            DoIPMessage::read(&mut buf.as_ref()).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 8);
        let buf: [u8; 15] = [
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message: DoIPMessage<uds_protocol::ProtocolResponse> =
            DoIPMessage::read(&mut buf.as_ref()).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2010);
        assert!(
            deserialized_message.header.payload_type == PayloadType::VehicleIdentificationRequest
        );
        assert!(deserialized_message.header.payload_length == 7);
        // TODO: Lots more checking of payload handling
        //assert!(deserialized_message.payload.len() == 7);
    }
    #[test]
    fn test_invalid_inverse() {
        let buf: [u8; 8] = [0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // This parsing should panic for the bad inverse
        // TODO: Instead of panicking need to add error handling and return a result

        assert!(matches!(
            DoIPMessage::<uds_protocol::ProtocolRequest>::read(&mut buf.as_ref()),
            Err(DoIPMessageError::VersionInverseIncorrect { .. })
        ));
    }
}
