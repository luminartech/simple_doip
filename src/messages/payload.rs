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
    /// Vehicle announcement / vehicle identification response (`PayloadType::VehicleAnnouncement`,
    /// 0x0004). Shares the [`VehicleIdentificationResponse`] wire format.
    VehicleAnnouncement(VehicleIdentificationResponse),
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
            Payload::VehicleAnnouncement(response) => f
                .debug_tuple("VehicleAnnouncement")
                .field(response)
                .finish(),
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

impl<D: AsRef<[u8]>> Encode for Payload<D> {
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
