pub mod alive_check_response;
pub mod diagnostic_message;
pub mod diagnostic_message_ack;
pub mod entity_status_response;
pub mod header;
pub mod message_error;
pub mod nack;
pub mod power_mode_info_response;
pub mod routing_activation_request;
pub mod routing_activation_response;
pub mod vehicle_identification_response;

use std::io::Write;

use crate::messages::{
    alive_check_response::AliveCheckResponse, diagnostic_message::DiagnosticMessage,
    entity_status_response::EntityStatusResponse, header::DoIPHeader, header::PayloadType,
    message_error::DoIPMessageError, nack::NackCode,
    power_mode_info_response::DiagnosticPowerModeCode,
    routing_activation_request::RoutingActivationRequest,
    routing_activation_response::RoutingActivationResponse,
    vehicle_identification_response::VehicleIdentificationResponse,
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
    /// DoIP Diagnostic Message Negative Acknowledge
    DiagnosticMessageNegativeAcknowledge(DiagnosticMessageAck),
    /// DoIP Spec Reserved
    Reserved(u16),
    /// DoIP Spec Reserved for Vehicle Manufacturer
    ReservedVehicleManufacturer(u16),
}
pub struct DoIPMessage {
    pub header: header::DoIPHeader,
    pub payload: DoIPPayload,
}

impl DoIPMessage {
    // TODO: This needs careful review and should do a lot more error checking than it does now
    pub fn read(message_bytes: &mut [u8]) -> Result<DoIPMessage, DoIPMessageError> {
        let header = DoIPHeader::read(&mut message_bytes.as_ref());
        header.version_inverse_correct()?;
        let payload = &message_bytes[8..];
        if header.payload_length != payload.len() as u32 {
            return Err(DoIPMessageError::PayloadLengthIncorrect {
                value: payload.len(),
                expected: header.payload_length,
            });
        }
        let payload = match header.payload_type {
            PayloadType::NegativeAcknowledge => {
                if payload.len() != 1 {
                    return Err(DoIPMessageError::PayloadLengthIncorrect {
                        value: payload.len(),
                        expected: 1,
                    });
                }
                DoIPPayload::NegativeAcknowledge(NackCode::from(payload[0]))
            }
            PayloadType::VehicleIdentificationRequest => DoIPPayload::VehicleIdentificationRequest,
            PayloadType::VehicleIdentificationRequestWithEID => {
                let eid: [u8; 6] =
                    payload
                        .try_into()
                        .map_err(|e| DoIPMessageError::PayloadLengthIncorrect {
                            value: payload.len(),
                            expected: 6,
                        })?;
                DoIPPayload::VehicleIdentificationRequestWithEID(eid)
            }
            PayloadType::VehicleIdentificationRequestWithVIN => {
                let vin: [u8; 17] =
                    payload
                        .try_into()
                        .map_err(|e| DoIPMessageError::PayloadLengthIncorrect {
                            value: payload.len(),
                            expected: 17,
                        })?;
                DoIPPayload::VehicleIdentificationRequestWithVIN(vin)
            }
            PayloadType::VehicleAnnouncement => DoIPPayload::VehicleAnnouncement(
                VehicleIdentificationResponse::read(&mut payload.as_ref())?,
            ),
            PayloadType::RoutingActivationRequest => DoIPPayload::RoutingActivationRequest(
                RoutingActivationRequest::read(&mut payload.as_ref())?,
            ),
            PayloadType::RoutingActivationResponse => DoIPPayload::RoutingActivationResponse(
                RoutingActivationResponse::read(&mut payload.as_ref(), payload.len())?,
            ),
            PayloadType::AliveCheckRequest => DoIPPayload::AliveCheckRequest,
            PayloadType::AliveCheckResponse => {
                DoIPPayload::AliveCheckResponse(AliveCheckResponse::read(&mut payload.as_ref())?)
            }
            PayloadType::DoIPEntityStatusRequest => DoIPPayload::DoIPEntityStatusRequest,
            PayloadType::DoIPEntityStatusResponse => DoIPPayload::DoIPEntityStatusResponse(
                EntityStatusResponse::read(&mut payload.as_ref())?,
            ),
            PayloadType::DiagnosticPowerModeInfoRequest => {
                DoIPPayload::DiagnosticPowerModeInfoRequest
            }
            PayloadType::DiagnosticPowerModeInfoResponse => {
                if payload.len() != 1 {
                    return Err(DoIPMessageError::PayloadLengthIncorrect {
                        value: payload.len(),
                        expected: 1,
                    });
                }
                DoIPPayload::DiagnosticPowerModeInfoResponse(DiagnosticPowerModeCode::from(
                    payload[0],
                ))
            }
            PayloadType::DiagnosticMessage => DoIPPayload::DiagnosticMessage(
                DiagnosticMessage::read(&mut payload.as_ref(), payload.len())?,
            ),
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                DoIPPayload::DiagnosticMessagePositiveAcknowledge(DiagnosticMessageAck::read(
                    &mut payload.as_ref(),
                    payload.len(),
                )?)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => {
                DoIPPayload::DiagnosticMessageNegativeAcknowledge(DiagnosticMessageAck::read(
                    &mut payload.as_ref(),
                    payload.len(),
                )?)
            }

            PayloadType::Reserved(value) => DoIPPayload::Reserved(value),
            PayloadType::ReservedVehicleManufacturer(u16) => {
                DoIPPayload::ReservedVehicleManufacturer(u16)
            }
        };

        Ok(DoIPMessage { header, payload })
    }

    pub fn write<T: Write>(&self, writer: &mut T) {
        self.header.write(writer);
        match self.header.payload_type {
            PayloadType::NegativeAcknowledge => {
                if payload.len() != 1 {
                    return Err(DoIPMessageError::PayloadLengthIncorrect {
                        value: payload.len(),
                        expected: 1,
                    });
                }
                DoIPPayload::NegativeAcknowledge(NackCode::from(payload[0]))
            }
            PayloadType::VehicleIdentificationRequest => DoIPPayload::VehicleIdentificationRequest,
            PayloadType::VehicleIdentificationRequestWithEID => {
                let eid: [u8; 6] =
                    payload
                        .try_into()
                        .map_err(|e| DoIPMessageError::PayloadLengthIncorrect {
                            value: payload.len(),
                            expected: 6,
                        })?;
                DoIPPayload::VehicleIdentificationRequestWithEID(eid)
            }
            PayloadType::VehicleIdentificationRequestWithVIN => {
                let vin: [u8; 17] =
                    payload
                        .try_into()
                        .map_err(|e| DoIPMessageError::PayloadLengthIncorrect {
                            value: payload.len(),
                            expected: 17,
                        })?;
                DoIPPayload::VehicleIdentificationRequestWithVIN(vin)
            }
            PayloadType::VehicleAnnouncement => DoIPPayload::VehicleAnnouncement(
                VehicleIdentificationResponse::read(&mut payload.as_ref())?,
            ),
            PayloadType::RoutingActivationRequest => DoIPPayload::RoutingActivationRequest(
                RoutingActivationRequest::read(&mut payload.as_ref())?,
            ),
            PayloadType::RoutingActivationResponse => DoIPPayload::RoutingActivationResponse(
                RoutingActivationResponse::read(&mut payload.as_ref(), payload.len())?,
            ),
            PayloadType::AliveCheckRequest => DoIPPayload::AliveCheckRequest,
            PayloadType::AliveCheckResponse => {
                DoIPPayload::AliveCheckResponse(AliveCheckResponse::read(&mut payload.as_ref())?)
            }
            PayloadType::DoIPEntityStatusRequest => DoIPPayload::DoIPEntityStatusRequest,
            PayloadType::DoIPEntityStatusResponse => DoIPPayload::DoIPEntityStatusResponse(
                EntityStatusResponse::read(&mut payload.as_ref())?,
            ),
            PayloadType::DiagnosticPowerModeInfoRequest => {
                DoIPPayload::DiagnosticPowerModeInfoRequest
            }
            PayloadType::DiagnosticPowerModeInfoResponse => {
                if payload.len() != 1 {
                    return Err(DoIPMessageError::PayloadLengthIncorrect {
                        value: payload.len(),
                        expected: 1,
                    });
                }
                DoIPPayload::DiagnosticPowerModeInfoResponse(DiagnosticPowerModeCode::from(
                    payload[0],
                ))
            }
            PayloadType::DiagnosticMessage => DoIPPayload::DiagnosticMessage(
                DiagnosticMessage::read(&mut payload.as_ref(), payload.len())?,
            ),
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                DoIPPayload::DiagnosticMessagePositiveAcknowledge(DiagnosticMessageAck::read(
                    &mut payload.as_ref(),
                    payload.len(),
                )?)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => {
                DoIPPayload::DiagnosticMessageNegativeAcknowledge(DiagnosticMessageAck::read(
                    &mut payload.as_ref(),
                    payload.len(),
                )?)
            }

            PayloadType::Reserved(value) => DoIPPayload::Reserved(value),
            PayloadType::ReservedVehicleManufacturer(u16) => {
                DoIPPayload::ReservedVehicleManufacturer(u16)
            }
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
