use std::io::{Read, Write};

use uds_protocol::SingleValueWireFormat;

use crate::messages::{
    AliveCheckResponse, DiagnosticMessage, DiagnosticMessageAck, DiagnosticPowerModeCode,
    DoIPMessageError, EntityStatusResponse, PayloadType, RoutingActivationResponse,
    VehicleIdentificationResponse,
};

use super::{NackCode, RoutingActivationRequest};

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload<DiagnosticsDefinition> {
    DoIPNack(NackCode),
    AliveCheckRequest,
    AliveCheckResponse(AliveCheckResponse),
    DiagnosticMessage(DiagnosticMessage<DiagnosticsDefinition>),
    DiagnosticMessageAck(DiagnosticMessageAck),
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

impl<DiagnosticsDefinitions: SingleValueWireFormat> Payload<DiagnosticsDefinitions> {
    pub fn read<R: Read>(
        mut payload_bytes: &mut R,
        payload_type: PayloadType,
    ) -> Result<Self, DoIPMessageError> {
        Ok(match payload_type {
            PayloadType::AliveCheckResponse => {
                Self::AliveCheckResponse(AliveCheckResponse::read(&mut payload_bytes)?)
            }
            PayloadType::NegativeAcknowledge => Self::DoIPNack(NackCode::read(&mut payload_bytes)?),
            PayloadType::VehicleIdentificationRequest => Self::VehicleIdentificationRequest,
            PayloadType::VehicleIdentificationRequestWithEID => Self::VehicleIdentificationRequest,
            PayloadType::VehicleIdentificationRequestWithVIN => Self::VehicleIdentificationRequest,
            PayloadType::VehicleAnnouncement => Self::VehicleAnnouncement,
            PayloadType::RoutingActivationRequest => {
                Self::RoutingActivationRequest(RoutingActivationRequest::read(payload_bytes)?)
            }
            PayloadType::RoutingActivationResponse => {
                Self::RoutingActivationResponse(RoutingActivationResponse::read(payload_bytes)?)
            }
            PayloadType::AliveCheckRequest => Self::AliveCheckRequest,
            PayloadType::DoIPEntityStatusRequest => todo!(),
            PayloadType::DoIPEntityStatusResponse => todo!(),
            PayloadType::DiagnosticPowerModeInfoRequest => todo!(),
            PayloadType::DiagnosticPowerModeInfoResponse => todo!(),
            PayloadType::DiagnosticMessage => {
                Self::DiagnosticMessage(DiagnosticMessage::read(payload_bytes)?)
            }
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                Self::DiagnosticMessageAck(DiagnosticMessageAck::read(payload_bytes)?)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => Self::DiagnosticMessageNack,
            PayloadType::Reserved(_) => todo!(),
            PayloadType::ReservedVehicleManufacturer(_) => todo!(),
        })
    }

    pub fn write<W: Write>(&self, writer: &mut W) -> Result<usize, DoIPMessageError> {
        Ok(match self {
            Payload::DoIPNack(nack) => nack.write(writer)?,
            Payload::AliveCheckRequest => 0,
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.write(writer)?
            }
            Payload::DiagnosticMessage(diagnostic_message) => diagnostic_message.write(writer)?,
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.write(writer)?
            }
            Payload::DiagnosticMessageNack => 0,
            Payload::EntityStatusRequest => 0,
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.write(writer)?
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.write(writer)?
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.write(writer)?
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.write(writer)?
            }
            Payload::VehicleIdentificationRequest => 0,
            Payload::VehicleIdentificationResponse(vehicle_identification_response) => {
                vehicle_identification_response.write(writer)?
            }
            Payload::VehicleAnnouncement => todo!(),
        })
    }
}
