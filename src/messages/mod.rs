mod alive_check_response;
pub use alive_check_response::AliveCheckResponse;
mod diagnostic_message;
pub use diagnostic_message::DiagnosticMessage;
mod diagnostic_message_ack;
pub use diagnostic_message_ack::{DiagnosticAckCode, DiagnosticMessageAck};
mod entity_status_response;
pub use entity_status_response::{EntityStatusNodeType, EntityStatusResponse};
mod header;
pub use header::{Header, PayloadType, ProtocolVersion};
mod message_error;
pub use message_error::MessageError;
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
pub use vehicle_identification_response::{
    FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
};

use std::io::{Read, Write};
use uds_protocol::WireFormat;

use crate::LogicalAddress;

/// Message contains the payload and header info of a DoIP message
///
/// The payload is a generic type that implements the WireFormat trait
/// The header is a fixed size struct that contains the protocol version, payload type, and payload length
#[derive(Debug, Clone, PartialEq)]
pub struct Message<W> {
    pub header: header::Header,
    pub payload: Payload<W>,
}

impl<W: WireFormat> Message<W> {
    pub fn is_response(&self, payload_type: PayloadType) -> bool {
        match self.header.payload_type {
            PayloadType::RoutingActivationRequest => {
                payload_type == PayloadType::RoutingActivationResponse
            }
            PayloadType::AliveCheckRequest => payload_type == PayloadType::AliveCheckResponse,
            PayloadType::DiagnosticMessage => {
                // DiagnosticMessage can be a request or response in certain models
                payload_type == PayloadType::DiagnosticMessageNegativeAcknowledge
                    || payload_type == PayloadType::DiagnosticMessagePositiveAcknowledge
                    || payload_type == PayloadType::DiagnosticMessage
            }
            PayloadType::DoIPEntityStatusRequest => {
                payload_type == PayloadType::DoIPEntityStatusResponse
            }
            PayloadType::DiagnosticPowerModeInfoRequest => {
                payload_type == PayloadType::DiagnosticPowerModeInfoResponse
            }
            PayloadType::VehicleIdentificationRequest
            | PayloadType::VehicleIdentificationRequestWithEID
            | PayloadType::VehicleIdentificationRequestWithVIN => {
                payload_type == PayloadType::VehicleAnnouncement
            }
            _ => false,
        }
    }

    pub fn alive_check_request(protocol_version: ProtocolVersion) -> Message<W> {
        Message {
            header: Header::new(protocol_version, PayloadType::AliveCheckRequest, 0),
            payload: Payload::AliveCheckRequest,
        }
    }

    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
    ) -> Message<W> {
        let response = AliveCheckResponse { source_address };
        Message {
            header: Header::new(protocol_version, PayloadType::AliveCheckResponse, 2),
            payload: Payload::AliveCheckResponse(response),
        }
    }

    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        message: W,
    ) -> Message<W> {
        let payload_size = message.required_size() as u32 + 4;
        let message = DiagnosticMessage {
            source_address,
            target_address,
            user_data: message,
        };
        Message {
            header: Header::new(
                protocol_version,
                PayloadType::DiagnosticMessage,
                payload_size,
            ),
            payload: Payload::DiagnosticMessage(message),
        }
    }

    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        ack_code: DiagnosticAckCode,
        previous_message_data: Vec<u8>,
    ) -> Message<W> {
        let ack = DiagnosticMessageAck {
            source_address,
            target_address,
            ack_code,
            previous_message_data,
        };
        Message {
            header: Header::new(
                protocol_version,
                PayloadType::DiagnosticMessagePositiveAcknowledge,
                5 + ack.previous_message_data.len() as u32,
            ),
            payload: Payload::DiagnosticMessageAck(ack),
        }
    }

    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Message<W> {
        let request = RoutingActivationRequest {
            source_address,
            activation_type,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer,
        };

        let mut payload = Vec::with_capacity(11);
        request.write(&mut payload).unwrap();

        let header = Header::new(
            protocol_version,
            PayloadType::RoutingActivationRequest,
            payload.len() as u32,
        );
        Message {
            header,
            payload: Payload::RoutingActivationRequest(request),
        }
    }

    pub fn routing_activation_response(
        protocol_version: ProtocolVersion,
        logical_address_tester: LogicalAddress,
        logical_address_of_doip_entity: LogicalAddress,
        routing_activation_response_code: RoutingActivationResponseCode,
        reserved_oem: [u8; 4],
        oem_specific: Option<[u8; 4]>,
    ) -> Message<W> {
        let response = RoutingActivationResponse {
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        };
        Message {
            // TODO: Check the payload length
            header: Header::new(protocol_version, PayloadType::RoutingActivationResponse, 9),
            payload: Payload::RoutingActivationResponse(response),
        }
    }
}

impl<W: WireFormat> Message<W> {
    // TODO: This needs careful review and should do a lot more error checking than it does now
    pub fn read<T: Read>(mut message_bytes: &mut T) -> Result<Message<W>, MessageError> {
        let header = Header::read(&mut message_bytes)?;
        header.version_inverse_correct()?;
        let payload = Payload::read(&mut message_bytes, header.payload_type)?;
        Ok(Message { header, payload })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        let mut written = self.header.write(writer)?;
        written += &self.payload.write(writer)?;
        Ok(written)
    }

    pub fn is_positive_response_suppressed(&self) -> bool {
        match &self.payload {
            Payload::DiagnosticMessage(diagnostic_message) => {
                diagnostic_message.is_positive_response_suppressed()
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use header::{PayloadType, ProtocolVersion};

    /// Check that we properly decode and encode hex bytes
    #[test]
    fn test_valid_messages() {
        let buf: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
        let deserialized_message: Message<uds_protocol::ProtocolRequest> =
            Message::read(&mut buf.as_ref()).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 1);
        let buf: [u8; 15] = [
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message: Message<uds_protocol::ProtocolResponse> =
            Message::read(&mut buf.as_ref()).unwrap();
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
            Message::<uds_protocol::ProtocolRequest>::read(&mut buf.as_ref()),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }
}
