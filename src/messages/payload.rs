use crate::messages::{
    AliveCheckResponse, DiagnosticMessage, DiagnosticMessageAck, DiagnosticPowerModeCode,
    EntityStatusResponse, MessageError, PayloadType, RoutingActivationResponse,
    VehicleIdentificationResponse,
};

use super::traits::{Decode, Encode};
use super::{NackCode, RoutingActivationRequest};

/// Maps [`PayloadType`] to the corresponding `Payload` type when reading and writing
/// messages. This is the main payload type for `DoIP` messages.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Payload<'a> {
    DoIPNack(NackCode),
    AliveCheckRequest,
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(DiagnosticMessage<'a>),
    DiagnosticMessageAck(DiagnosticMessageAck<'a>),
    DiagnosticMessageNack,
    EntityStatusRequest,
    EntityStatusResponse(EntityStatusResponse),
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    RoutingActivationRequest(RoutingActivationRequest),
    RoutingActivationResponse(RoutingActivationResponse),
    /// Vehicle announcement / vehicle identification response (`PayloadType::VehicleAnnouncement`,
    /// 0x0004). Shares the [`VehicleIdentificationResponse`] wire format.
    VehicleAnnouncement(VehicleIdentificationResponse),
    VehicleIdentificationRequest,
    VehicleIdentificationResponse(VehicleIdentificationResponse),
}

/// Owned mirror of [`Payload`] for values that must outlive an RX buffer (tokio
/// channels, `ServerConnectionHandler` responses). Only the two data-carrying leaf
/// variants need owned storage; every other variant is already fully owned.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OwnedPayload {
    DoIPNack(NackCode),
    AliveCheckRequest,
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(super::OwnedDiagnosticMessage),
    DiagnosticMessageAck(super::OwnedDiagnosticMessageAck),
    DiagnosticMessageNack,
    EntityStatusRequest,
    EntityStatusResponse(EntityStatusResponse),
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    RoutingActivationRequest(RoutingActivationRequest),
    RoutingActivationResponse(RoutingActivationResponse),
    /// Vehicle announcement / vehicle identification response (`PayloadType::VehicleAnnouncement`,
    /// 0x0004). Shares the [`VehicleIdentificationResponse`] wire format.
    VehicleAnnouncement(VehicleIdentificationResponse),
    VehicleIdentificationRequest,
    VehicleIdentificationResponse(VehicleIdentificationResponse),
}

#[cfg(feature = "alloc")]
impl Payload<'_> {
    /// Copy any borrowed payload data into an owned payload.
    #[must_use]
    pub fn to_owned_payload(&self) -> OwnedPayload {
        match self {
            Payload::DoIPNack(nack) => OwnedPayload::DoIPNack(*nack),
            Payload::AliveCheckRequest => OwnedPayload::AliveCheckRequest,
            Payload::AliveCheckResponse(response) => OwnedPayload::AliveCheckResponse(*response),
            Payload::DiagnosticMessage(message) => {
                OwnedPayload::DiagnosticMessage(message.to_owned_message())
            }
            Payload::DiagnosticMessageAck(ack) => {
                OwnedPayload::DiagnosticMessageAck(ack.to_owned_message())
            }
            Payload::DiagnosticMessageNack => OwnedPayload::DiagnosticMessageNack,
            Payload::EntityStatusRequest => OwnedPayload::EntityStatusRequest,
            Payload::EntityStatusResponse(response) => {
                OwnedPayload::EntityStatusResponse(*response)
            }
            Payload::PowerModeInfoResponse(code) => OwnedPayload::PowerModeInfoResponse(*code),
            Payload::RoutingActivationRequest(request) => {
                OwnedPayload::RoutingActivationRequest(*request)
            }
            Payload::RoutingActivationResponse(response) => {
                OwnedPayload::RoutingActivationResponse(*response)
            }
            Payload::VehicleAnnouncement(response) => OwnedPayload::VehicleAnnouncement(*response),
            Payload::VehicleIdentificationRequest => OwnedPayload::VehicleIdentificationRequest,
            Payload::VehicleIdentificationResponse(response) => {
                OwnedPayload::VehicleIdentificationResponse(*response)
            }
        }
    }
}

#[cfg(feature = "alloc")]
impl OwnedPayload {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> Payload<'_> {
        match self {
            OwnedPayload::DoIPNack(nack) => Payload::DoIPNack(*nack),
            OwnedPayload::AliveCheckRequest => Payload::AliveCheckRequest,
            OwnedPayload::AliveCheckResponse(response) => Payload::AliveCheckResponse(*response),
            OwnedPayload::DiagnosticMessage(message) => {
                Payload::DiagnosticMessage(message.as_ref())
            }
            OwnedPayload::DiagnosticMessageAck(ack) => Payload::DiagnosticMessageAck(ack.as_ref()),
            OwnedPayload::DiagnosticMessageNack => Payload::DiagnosticMessageNack,
            OwnedPayload::EntityStatusRequest => Payload::EntityStatusRequest,
            OwnedPayload::EntityStatusResponse(response) => {
                Payload::EntityStatusResponse(*response)
            }
            OwnedPayload::PowerModeInfoResponse(code) => Payload::PowerModeInfoResponse(*code),
            OwnedPayload::RoutingActivationRequest(request) => {
                Payload::RoutingActivationRequest(*request)
            }
            OwnedPayload::RoutingActivationResponse(response) => {
                Payload::RoutingActivationResponse(*response)
            }
            OwnedPayload::VehicleAnnouncement(response) => Payload::VehicleAnnouncement(*response),
            OwnedPayload::VehicleIdentificationRequest => Payload::VehicleIdentificationRequest,
            OwnedPayload::VehicleIdentificationResponse(response) => {
                Payload::VehicleIdentificationResponse(*response)
            }
        }
    }
}

impl<'a> Payload<'a> {
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
            PayloadType::VehicleAnnouncement => {
                Self::VehicleAnnouncement(VehicleIdentificationResponse::decode(buf)?.0)
            }
            PayloadType::RoutingActivationRequest => {
                Self::RoutingActivationRequest(RoutingActivationRequest::decode(buf)?.0)
            }
            PayloadType::RoutingActivationResponse => {
                Self::RoutingActivationResponse(RoutingActivationResponse::decode(buf)?.0)
            }
            PayloadType::AliveCheckRequest => Self::AliveCheckRequest,
            PayloadType::DoIPEntityStatusRequest => Self::EntityStatusRequest,
            PayloadType::DoIPEntityStatusResponse => {
                Self::EntityStatusResponse(EntityStatusResponse::decode(buf)?.0)
            }
            PayloadType::DiagnosticPowerModeInfoResponse => {
                Self::PowerModeInfoResponse(DiagnosticPowerModeCode::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessage => {
                Self::DiagnosticMessage(DiagnosticMessage::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                Self::DiagnosticMessageAck(DiagnosticMessageAck::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => Self::DiagnosticMessageNack,
            // `DiagnosticPowerModeInfoRequest` has no dedicated `Payload` variant, and the
            // reserved ranges are not decodable. Return an error rather than panicking on
            // peer-controlled input.
            PayloadType::DiagnosticPowerModeInfoRequest
            | PayloadType::Reserved(_)
            | PayloadType::ReservedVehicleManufacturer(_) => {
                return Err(MessageError::UnsupportedPayloadType(payload_type));
            }
        })
    }
}

impl Encode for Payload<'_> {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(match self {
            Payload::DoIPNack(nack) => nack.encoded_size()?,
            Payload::AliveCheckRequest
            | Payload::DiagnosticMessageNack
            | Payload::EntityStatusRequest
            | Payload::VehicleIdentificationRequest => 0,
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.encoded_size()?
            }
            Payload::DiagnosticMessage(diagnostic_message) => diagnostic_message.encoded_size()?,
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.encoded_size()?
            }
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.encoded_size()?
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.encoded_size()?
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.encoded_size()?
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.encoded_size()?
            }
            // `VehicleAnnouncement` shares the `VehicleIdentificationResponse` wire format.
            Payload::VehicleIdentificationResponse(vehicle_identification_response)
            | Payload::VehicleAnnouncement(vehicle_identification_response) => {
                vehicle_identification_response.encoded_size()?
            }
        })
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
            // `VehicleAnnouncement` shares the `VehicleIdentificationResponse` wire format.
            Payload::VehicleIdentificationResponse(vehicle_identification_response)
            | Payload::VehicleAnnouncement(vehicle_identification_response) => {
                vehicle_identification_response.encode(writer)?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalAddress;
    use crate::messages::{FurtherActionRequired, VinGidSyncStatus};

    /// A peer sending a payload type we cannot decode (e.g. a reserved type) must return
    /// an error, never panic (regression test for `todo!()` decode arms).
    #[test]
    fn unsupported_payload_type_errors_not_panics() {
        // 0x0009 falls in the reserved range -> `PayloadType::Reserved`.
        let payload_type = PayloadType::from(0x0009u16);
        assert!(matches!(payload_type, PayloadType::Reserved(0x0009)));
        let result = Payload::decode(&[], payload_type);
        assert!(matches!(
            result,
            Err(MessageError::UnsupportedPayloadType(_))
        ));

        // The power-mode info *request* has no payload variant and must also error.
        let result = Payload::decode(&[], PayloadType::DiagnosticPowerModeInfoRequest);
        assert!(matches!(
            result,
            Err(MessageError::UnsupportedPayloadType(_))
        ));
    }

    /// Previously-`todo!()` decode arms are now implemented; verify they decode instead of
    /// panicking.
    #[test]
    fn entity_status_request_decodes_empty() {
        let payload = Payload::decode(&[], PayloadType::DoIPEntityStatusRequest).unwrap();
        assert!(matches!(payload, Payload::EntityStatusRequest));
    }

    /// `VehicleAnnouncement` must round-trip encode -> decode (regression test for the
    /// `todo!()` encode arms that panicked on a value decode had produced).
    #[test]
    fn vehicle_announcement_round_trips() {
        let response = VehicleIdentificationResponse {
            vin: [0x41; 17],
            logical_address: LogicalAddress(0x0E00),
            entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            group_id: Some([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        };
        let payload = Payload::VehicleAnnouncement(response);

        let mut buf = [0u8; 64];
        let written = {
            let mut writer: &mut [u8] = &mut buf;
            payload.encode(&mut writer).unwrap()
        };
        assert_eq!(written, payload.encoded_size().unwrap());

        let decoded = Payload::decode(&buf[..written], PayloadType::VehicleAnnouncement).unwrap();
        assert_eq!(decoded, payload);
    }
}
