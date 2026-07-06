use crate::messages::{
    AliveCheckResponse, DiagnosticMessage, DiagnosticMessageAck, DiagnosticPowerModeCode,
    EntityStatusResponse, MessageError, PayloadType, RoutingActivationResponse,
    VehicleIdentificationResponse,
};

use super::traits::{Decode, Encode};
use super::{NackCode, RoutingActivationRequest};

/// Maps [`PayloadType`] to the corresponding `Payload` type when reading and writing
/// messages. This is the main payload type for `DoIP` messages.
#[derive(Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum Payload<D> {
    DoIPNack(NackCode),
    AliveCheckRequest,
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(DiagnosticMessage<D>),
    DiagnosticMessageAck(DiagnosticMessageAck<D>),
    DiagnosticMessageNack,
    EntityStatusRequest,
    EntityStatusResponse(EntityStatusResponse),
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    RoutingActivationRequest(RoutingActivationRequest),
    RoutingActivationResponse(RoutingActivationResponse),
    VehicleAnnouncement,
    VehicleIdentificationRequest,
    VehicleIdentificationResponse(VehicleIdentificationResponse),
}

// A manual impl is required (rather than `#[derive(Debug)]`) because
// `DiagnosticMessageAck<D>`'s `Debug` impl needs `D: AsRef<[u8]>` (to hex-format the
// trailing data), a bound `#[derive(Debug)]` cannot infer automatically.
impl<D: AsRef<[u8]> + core::fmt::Debug> core::fmt::Debug for Payload<D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Payload::DoIPNack(nack) => f.debug_tuple("DoIPNack").field(nack).finish(),
            Payload::AliveCheckRequest => f.write_str("AliveCheckRequest"),
            Payload::AliveCheckResponse(response) => {
                f.debug_tuple("AliveCheckResponse").field(response).finish()
            }
            Payload::DiagnosticMessage(message) => {
                f.debug_tuple("DiagnosticMessage").field(message).finish()
            }
            Payload::DiagnosticMessageAck(ack) => {
                f.debug_tuple("DiagnosticMessageAck").field(ack).finish()
            }
            Payload::DiagnosticMessageNack => f.write_str("DiagnosticMessageNack"),
            Payload::EntityStatusRequest => f.write_str("EntityStatusRequest"),
            Payload::EntityStatusResponse(response) => f
                .debug_tuple("EntityStatusResponse")
                .field(response)
                .finish(),
            Payload::PowerModeInfoResponse(code) => {
                f.debug_tuple("PowerModeInfoResponse").field(code).finish()
            }
            Payload::RoutingActivationRequest(request) => f
                .debug_tuple("RoutingActivationRequest")
                .field(request)
                .finish(),
            Payload::RoutingActivationResponse(response) => f
                .debug_tuple("RoutingActivationResponse")
                .field(response)
                .finish(),
            Payload::VehicleAnnouncement => f.write_str("VehicleAnnouncement"),
            Payload::VehicleIdentificationRequest => f.write_str("VehicleIdentificationRequest"),
            Payload::VehicleIdentificationResponse(response) => f
                .debug_tuple("VehicleIdentificationResponse")
                .field(response)
                .finish(),
        }
    }
}

impl<'a> Payload<&'a [u8]> {
    /// Decode a payload of the given type from exactly the payload bytes of one message.
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the payload cannot be deserialized
    pub fn decode(buf: &'a [u8], payload_type: PayloadType) -> Result<Self, MessageError> {
        Ok(match payload_type {
            PayloadType::AliveCheckResponse => {
                Self::AliveCheckResponse(AliveCheckResponse::decode(buf)?.0)
            }
            PayloadType::NegativeAcknowledge => Self::DoIPNack(NackCode::decode(buf)?.0),
            PayloadType::VehicleIdentificationRequest
            | PayloadType::VehicleIdentificationRequestWithEID
            | PayloadType::VehicleIdentificationRequestWithVIN => {
                Self::VehicleIdentificationRequest
            }
            PayloadType::VehicleAnnouncement => Self::VehicleAnnouncement,
            PayloadType::RoutingActivationRequest => {
                Self::RoutingActivationRequest(RoutingActivationRequest::decode(buf)?.0)
            }
            PayloadType::RoutingActivationResponse => {
                Self::RoutingActivationResponse(RoutingActivationResponse::decode(buf)?.0)
            }
            PayloadType::AliveCheckRequest => Self::AliveCheckRequest,
            PayloadType::DoIPEntityStatusRequest => todo!(),
            PayloadType::DoIPEntityStatusResponse => todo!(),
            PayloadType::DiagnosticPowerModeInfoRequest => todo!(),
            PayloadType::DiagnosticPowerModeInfoResponse => todo!(),
            PayloadType::DiagnosticMessage => {
                Self::DiagnosticMessage(DiagnosticMessage::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                Self::DiagnosticMessageAck(DiagnosticMessageAck::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => Self::DiagnosticMessageNack,
            PayloadType::Reserved(_) => todo!(),
            PayloadType::ReservedVehicleManufacturer(_) => todo!(),
        })
    }
}

impl<D: AsRef<[u8]>> Encode for Payload<D> {
    fn encoded_size(&self) -> usize {
        match self {
            Payload::DoIPNack(nack) => nack.encoded_size(),
            Payload::AliveCheckRequest
            | Payload::DiagnosticMessageNack
            | Payload::EntityStatusRequest
            | Payload::VehicleIdentificationRequest => 0,
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.encoded_size()
            }
            Payload::DiagnosticMessage(diagnostic_message) => diagnostic_message.encoded_size(),
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.encoded_size()
            }
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.encoded_size()
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.encoded_size()
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.encoded_size()
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.encoded_size()
            }
            Payload::VehicleIdentificationResponse(vehicle_identification_response) => {
                vehicle_identification_response.encoded_size()
            }
            Payload::VehicleAnnouncement => todo!(),
        }
    }

    /// Serialize this payload into `writer`
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the payload cannot be serialized
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        Ok(match self {
            Payload::DoIPNack(nack) => nack.encode(writer)?,
            Payload::AliveCheckRequest
            | Payload::DiagnosticMessageNack
            | Payload::EntityStatusRequest
            | Payload::VehicleIdentificationRequest => 0,
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.encode(writer)?
            }
            Payload::DiagnosticMessage(diagnostic_message) => diagnostic_message.encode(writer)?,
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.encode(writer)?
            }
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.encode(writer)?
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.encode(writer)?
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.encode(writer)?
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.encode(writer)?
            }
            Payload::VehicleIdentificationResponse(vehicle_identification_response) => {
                vehicle_identification_response.encode(writer)?
            }
            Payload::VehicleAnnouncement => todo!(),
        })
    }
}
