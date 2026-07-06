mod alive_check_response;
pub use alive_check_response::AliveCheckResponse;
mod decode_util;
mod diagnostic_message;
mod encode_util;
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
mod traits;
pub use traits::{Decode, Encode};
mod vehicle_identification_response;
pub use vehicle_identification_response::{
    FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
};

use crate::LogicalAddress;

/// Message contains the payload and header info of a `DoIP` message
///
/// The payload contains diagnostic data and other `DoIP` protocol information
/// The header is a fixed size struct that contains the protocol version, payload type, and payload length
#[derive(Clone, PartialEq)]
pub struct Message<D> {
    pub header: header::Header,
    pub payload: Payload<D>,
}

// A manual impl is required (rather than `#[derive(Debug)]`) because `Payload<D>`'s
// `Debug` impl needs `D: AsRef<[u8]>`, a bound `#[derive(Debug)]` cannot infer
// automatically.
impl<D: AsRef<[u8]> + core::fmt::Debug> core::fmt::Debug for Message<D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Message")
            .field("header", &self.header)
            .field("payload", &self.payload)
            .finish()
    }
}

/// Zero-copy message borrowing its data from an RX buffer.
pub type MessageRef<'a> = Message<&'a [u8]>;
#[cfg(feature = "alloc")]
pub type OwnedMessage = Message<alloc::vec::Vec<u8>>;

impl<D> Message<D> {
    /// Check whether the given payload type is a valid response to this message
    #[must_use]
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

    /// Construct an alive check request message
    #[must_use]
    pub fn alive_check_request(protocol_version: ProtocolVersion) -> Message<D> {
        Message {
            header: Header::new(protocol_version, PayloadType::AliveCheckRequest, 0),
            payload: Payload::AliveCheckRequest,
        }
    }

    /// Construct an alive check response message
    #[must_use]
    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
    ) -> Message<D> {
        let response = AliveCheckResponse { source_address };
        Message {
            header: Header::new(protocol_version, PayloadType::AliveCheckResponse, 2),
            payload: Payload::AliveCheckResponse(response),
        }
    }

    /// Construct a diagnostic message carrying opaque user data
    #[must_use]
    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        message: D,
    ) -> Message<D>
    where
        D: AsRef<[u8]>,
    {
        #[allow(clippy::cast_possible_truncation)]
        let payload_size = message.as_ref().len() as u32 + 4;
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

    /// Construct a diagnostic message acknowledgement
    #[must_use]
    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        ack_code: DiagnosticAckCode,
        previous_message_data: D,
    ) -> Message<D>
    where
        D: AsRef<[u8]>,
    {
        #[allow(clippy::cast_possible_truncation)]
        let payload_size = 5 + previous_message_data.as_ref().len() as u32;
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
                payload_size,
            ),
            payload: Payload::DiagnosticMessageAck(ack),
        }
    }

    /// Construct a routing activation request message
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Message<D> {
        let request = RoutingActivationRequest {
            source_address,
            activation_type,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer,
        };

        let header = Header::new(
            protocol_version,
            PayloadType::RoutingActivationRequest,
            request.encoded_size() as u32,
        );
        Message {
            header,
            payload: Payload::RoutingActivationRequest(request),
        }
    }

    /// Construct a routing activation response message
    #[must_use]
    pub fn routing_activation_response(
        protocol_version: ProtocolVersion,
        logical_address_tester: LogicalAddress,
        logical_address_of_doip_entity: LogicalAddress,
        routing_activation_response_code: RoutingActivationResponseCode,
        reserved_oem: [u8; 4],
        oem_specific: Option<[u8; 4]>,
    ) -> Message<D> {
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

impl<'a> Decode<'a> for Message<&'a [u8]> {
    /// Deserialize a complete `DoIP` message (header + payload) from a byte slice
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the header or payload cannot be deserialized
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (header, rest) = Header::decode(buf)?;
        let (payload_bytes, rest) = decode_util::take(rest, header.payload_length as usize)?;
        let payload = Payload::decode(payload_bytes, header.payload_type)?;
        Ok((Message { header, payload }, rest))
    }
}

impl<D: AsRef<[u8]>> Encode for Message<D> {
    fn encoded_size(&self) -> usize {
        Header::SIZE + self.payload.encoded_size()
    }

    /// Serialize this message (header + payload) into `writer`
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the header or payload cannot be serialized
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        let written = self.header.encode(writer)?;
        Ok(written + self.payload.encode(writer)?)
    }
}

#[cfg(feature = "alloc")]
impl Message<&[u8]> {
    /// Copy borrowed payload data into an owned message.
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedMessage {
        let payload = match &self.payload {
            Payload::DoIPNack(nack) => Payload::DoIPNack(*nack),
            Payload::AliveCheckRequest => Payload::AliveCheckRequest,
            Payload::AliveCheckResponse(response) => Payload::AliveCheckResponse(*response),
            Payload::DiagnosticMessage(message) => Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: message.source_address,
                target_address: message.target_address,
                user_data: message.user_data.to_vec(),
            }),
            Payload::DiagnosticMessageAck(ack) => {
                Payload::DiagnosticMessageAck(DiagnosticMessageAck {
                    source_address: ack.source_address,
                    target_address: ack.target_address,
                    ack_code: ack.ack_code,
                    previous_message_data: ack.previous_message_data.to_vec(),
                })
            }
            Payload::DiagnosticMessageNack => Payload::DiagnosticMessageNack,
            Payload::EntityStatusRequest => Payload::EntityStatusRequest,
            Payload::EntityStatusResponse(response) => Payload::EntityStatusResponse(*response),
            Payload::PowerModeInfoResponse(code) => Payload::PowerModeInfoResponse(*code),
            Payload::RoutingActivationRequest(request) => {
                Payload::RoutingActivationRequest(*request)
            }
            Payload::RoutingActivationResponse(response) => {
                Payload::RoutingActivationResponse(*response)
            }
            Payload::VehicleAnnouncement => Payload::VehicleAnnouncement,
            Payload::VehicleIdentificationRequest => Payload::VehicleIdentificationRequest,
            Payload::VehicleIdentificationResponse(response) => {
                Payload::VehicleIdentificationResponse(*response)
            }
        };
        Message {
            header: self.header.clone(),
            payload,
        }
    }
}

impl<D: Default> Default for Message<D> {
    /// Create a default diagnostic message (used as a placeholder when waiting for any response)
    fn default() -> Self {
        Message {
            header: Header::new(ProtocolVersion::V2012, PayloadType::DiagnosticMessage, 0),
            payload: Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: LogicalAddress(0),
                target_address: LogicalAddress(0),
                user_data: D::default(),
            }),
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
        let deserialized_message: MessageRef = MessageRef::decode(&buf).unwrap().0;
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 1);
        let buf: [u8; 15] = [
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message: MessageRef = MessageRef::decode(&buf).unwrap().0;
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
        // This parsing should error for the bad inverse

        assert!(matches!(
            MessageRef::decode(&buf),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }

    /// Encode a message into a stack buffer and frame it back out again, without any
    /// `Vec`/`alloc` involved. Exercises the no_std / no_alloc API surface.
    #[test]
    fn test_no_std_stack_buffer_roundtrip() {
        let message: MessageRef = Message::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            &[0x10u8, 0x02][..],
        );

        let mut buf = [0u8; 64];
        let written = {
            let mut writer: &mut [u8] = &mut buf;
            message.encode(&mut writer).unwrap()
        };

        let (decoded, consumed) = crate::try_frame(&buf[..written]).unwrap().unwrap();
        assert_eq!(consumed, written);
        assert_eq!(decoded, message);
    }
}
