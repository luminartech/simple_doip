use std::io::{Read, Write};

use uds_protocol::WireFormat;

use crate::messages::{
    AliveCheckResponse, DiagnosticMessage, DiagnosticMessageAck, DiagnosticPowerModeCode,
    DoIPMessageError, EntityStatusResponse, PayloadType, RoutingActivationResponse,
    VehicleIdentificationResponse,
};

use super::RoutingActivationRequest;

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Payload<DiagnosticsDefinition> {
    NoPayload,
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(DiagnosticMessage<DiagnosticsDefinition>),
    DiagnosticMessageAck(DiagnosticMessageAck),
    EntityStatusResponse(EntityStatusResponse),
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    RoutingActivationRequest(RoutingActivationRequest),
    RoutingActivationResponse(RoutingActivationResponse),
    VehicleAnnouncementResponse(VehicleIdentificationResponse),
}

impl<DiagnosticsDefinitions: WireFormat> Payload<DiagnosticsDefinitions> {
    pub fn read<R: Read>(
        mut payload_bytes: &mut R,
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

    pub fn write<W: Write>(&self, writer: &mut W) -> Result<usize, DoIPMessageError> {
        match self {
            Self::AliveCheckResponse(alive_check_response) => alive_check_response.write(writer),
            _ => Err(DoIPMessageError::UnexpectedPayloadType(
                PayloadType::AliveCheckResponse,
            )),
        }
    }
}
