use std::io::{Read, Write};

use crate::messages::{
    alive_check_response::AliveCheckResponse, diagnostic_message::DiagnosticMessage,
    diagnostic_message_ack::DiagnosticMessageAck, entity_status_response::EntityStatusResponse,
    header::PayloadType, message_error::DoIPMessageError,
    power_mode_info_response::DiagnosticPowerModeCode,
    routing_activation_response::RoutingActivationResponse,
    vehicle_identification_response::VehicleIdentificationResponse,
};

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Payload {
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(DiagnosticMessage),
    DiagnosticMessageAck(DiagnosticMessageAck),
    EntityStatusResponse(EntityStatusResponse),
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    RoutingActivationResponse(RoutingActivationResponse),
    VehicleAnnouncementResponse(VehicleIdentificationResponse),
}

impl Payload {
    pub fn read<T: Read>(
        mut payload_bytes: &mut T,
        payload_type: PayloadType,
    ) -> Result<Self, DoIPMessageError> {
        match payload_type {
            PayloadType::AliveCheckResponse => {
                let alive_check_response = AliveCheckResponse::read(&mut payload_bytes)?;
                Ok(Self::AliveCheckResponse(alive_check_response))
            }
            _ => Err(DoIPMessageError::UnexpectedPayloadType(payload_type)),
        }
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        match self {
            Self::AliveCheckResponse(alive_check_response) => alive_check_response.write(writer),
            _ => Err(DoIPMessageError::UnexpectedPayloadType(
                PayloadType::AliveCheckResponse,
            )),
        }
    }
}
