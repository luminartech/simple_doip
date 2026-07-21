//! `DoIP` message types: the generic header, the [`Payload`] enum covering every
//! `DoIP` payload type this crate supports, and the concrete request/response
//! structs for each one, per ISO 13400-2.

mod alive_check_response;
pub use alive_check_response::AliveCheckResponse;
mod diagnostic_message;
pub use diagnostic_message::DiagnosticMessage;
#[cfg(feature = "alloc")]
pub use diagnostic_message::OwnedDiagnosticMessage;
mod diagnostic_message_ack;
#[cfg(feature = "alloc")]
pub use diagnostic_message_ack::OwnedDiagnosticMessageAck;
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
#[cfg(feature = "alloc")]
pub use payload::OwnedPayload;
pub use payload::Payload;
mod power_mode_info_response;
pub use power_mode_info_response::DiagnosticPowerModeCode;
mod routing_activation_request;
pub use routing_activation_request::{ActivationTypeCode, RoutingActivationRequest};
mod routing_activation_response;
pub use routing_activation_response::{RoutingActivationResponse, RoutingActivationResponseCode};
mod traits;
use traits::take;
pub use traits::{Decode, Encode};
mod vehicle_identification_response;
pub use vehicle_identification_response::{
    FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
};

use crate::LogicalAddress;

/// Payload length for a `DoIP` header, from a payload's encoded size.
///
/// # Panics
/// Panics if the payload cannot be sized, or if its encoded size exceeds `u32::MAX`.
/// Neither is reachable for a well-formed `DoIP` message: every payload type has a
/// computable size, and the wire format caps payload length at `u32::MAX` by construction.
fn payload_len(value: &impl Encode<Error = MessageError>) -> u32 {
    u32::try_from(
        value
            .encoded_size()
            .expect("DoIP message is always sizable"),
    )
    .expect("DoIP payload length exceeds u32::MAX")
}

/// Message contains the payload and header info of a `DoIP` message
///
/// The payload contains diagnostic data and other `DoIP` protocol information.
/// The header is a fixed size struct that contains the protocol version, payload type,
/// and payload length. Payload data borrows from the RX buffer it was decoded from
/// (zero-copy); use `Message::to_owned_message` (requires the `alloc` feature)
/// to detach it.
#[derive(Clone, Debug, PartialEq)]
pub struct Message<'a> {
    /// The 8-byte `DoIP` generic header (protocol version, its inverse, payload
    /// type, and payload length) that describes `payload`.
    pub header: header::Header,
    /// The decoded, type-specific `DoIP` payload described by `header`.
    pub payload: Payload<'a>,
}

/// Fully owned `DoIP` message for values that must outlive an RX buffer (tokio
/// channels, spawned tasks, `ServerConnectionHandler` responses).
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedMessage {
    /// The 8-byte `DoIP` generic header (protocol version, its inverse, payload
    /// type, and payload length) that describes `payload`.
    pub header: header::Header,
    /// The decoded, type-specific `DoIP` payload described by `header`, owning
    /// any trailing data instead of borrowing from an RX buffer.
    pub payload: OwnedPayload,
}

impl<'a> Message<'a> {
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
    pub fn alive_check_request(protocol_version: ProtocolVersion) -> Message<'a> {
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
    ) -> Message<'a> {
        let response = AliveCheckResponse { source_address };
        Message {
            header: Header::new(protocol_version, PayloadType::AliveCheckResponse, 2),
            payload: Payload::AliveCheckResponse(response),
        }
    }

    /// Construct a diagnostic message carrying opaque user data
    ///
    /// # Panics
    /// Panics if `message.encoded_size()` errors, or if the resulting size does
    /// not fit in a `u32`. Neither is reachable here: `DiagnosticMessage::encoded_size`
    /// is pure arithmetic over the struct's own fields (no I/O to fail), and its
    /// result is `4 + user_data.len()` (the two 2-byte addresses plus the user
    /// data; the 8-byte header is deliberately excluded, since this value becomes
    /// the header's payload length), far below `u32::MAX` for any `user_data`
    /// slice that can exist in memory.
    #[must_use]
    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        user_data: &'a [u8],
    ) -> Message<'a> {
        let message = DiagnosticMessage {
            source_address,
            target_address,
            user_data,
        };
        let payload_size = payload_len(&message);
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
    ///
    /// # Known limitation
    /// The header is stamped with `PayloadType::DiagnosticMessagePositiveAcknowledge`
    /// (0x8002) regardless of `ack_code`, so a negative
    /// [`DiagnosticAckCode`] is currently emitted under the positive payload type
    /// instead of `DiagnosticMessageNegativeAcknowledge` (0x8003). This is a known
    /// open issue deferred to a follow-up change.
    ///
    /// # Panics
    /// Panics if `ack.encoded_size()` errors, or if the resulting size does not
    /// fit in a `u32`. Neither is reachable here: `DiagnosticMessageAck::encoded_size`
    /// is pure arithmetic over the struct's own fields (no I/O to fail), and its
    /// result is far below `u32::MAX` for any `previous_message_data` slice that
    /// can exist in memory.
    #[must_use]
    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        ack_code: DiagnosticAckCode,
        previous_message_data: &'a [u8],
    ) -> Message<'a> {
        let ack = DiagnosticMessageAck {
            source_address,
            target_address,
            ack_code,
            previous_message_data,
        };
        let payload_size = payload_len(&ack);
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
    ///
    /// # Panics
    /// Panics if `request.encoded_size()` errors, or if the resulting size does
    /// not fit in a `u32`. Neither is reachable here: `RoutingActivationRequest`
    /// encodes to either 7 or 11 bytes depending on whether the optional
    /// manufacturer-reserved tail is present (both well under `u32::MAX`), and
    /// `encoded_size` is pure arithmetic with no I/O to fail.
    #[must_use]
    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Message<'a> {
        let request = RoutingActivationRequest {
            source_address,
            activation_type,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer,
        };

        let header = Header::new(
            protocol_version,
            PayloadType::RoutingActivationRequest,
            payload_len(&request),
        );
        Message {
            header,
            payload: Payload::RoutingActivationRequest(request),
        }
    }

    /// Construct a routing activation response message
    ///
    /// # Panics
    /// Panics if `response.encoded_size()` errors, or if the resulting size does
    /// not fit in a `u32`. Neither is reachable here: `RoutingActivationResponse`
    /// encodes to either 9 or 13 bytes depending on whether the optional
    /// OEM-specific tail is present (both well under `u32::MAX`), and
    /// `encoded_size` is pure arithmetic with no I/O to fail.
    #[must_use]
    pub fn routing_activation_response(
        protocol_version: ProtocolVersion,
        logical_address_tester: LogicalAddress,
        logical_address_of_doip_entity: LogicalAddress,
        routing_activation_response_code: RoutingActivationResponseCode,
        reserved_oem: [u8; 4],
        oem_specific: Option<[u8; 4]>,
    ) -> Message<'a> {
        let response = RoutingActivationResponse {
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        };
        let header = Header::new(
            protocol_version,
            PayloadType::RoutingActivationResponse,
            payload_len(&response),
        );
        Message {
            header,
            payload: Payload::RoutingActivationResponse(response),
        }
    }
}

impl<'a> Decode<'a> for Message<'a> {
    type Error = MessageError;

    /// Deserialize a complete `DoIP` message (header + payload) from a byte slice
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the header or payload cannot be deserialized
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (header, rest) = Header::decode(buf)?;
        let (payload_bytes, rest) = take(rest, header.payload_length as usize)?;
        let payload = Payload::decode(payload_bytes, header.payload_type)?;
        Ok((Message { header, payload }, rest))
    }
}

impl Encode for Message<'_> {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(Header::SIZE + self.payload.encoded_size()?)
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
impl Message<'_> {
    /// Copy borrowed payload data into an owned message.
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedMessage {
        OwnedMessage {
            header: self.header.clone(),
            payload: self.payload.to_owned_payload(),
        }
    }
}

#[cfg(feature = "alloc")]
impl OwnedMessage {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> Message<'_> {
        Message {
            header: self.header.clone(),
            payload: self.payload.as_ref(),
        }
    }

    /// Check whether the given payload type is a valid response to this message
    #[must_use]
    pub fn is_response(&self, payload_type: PayloadType) -> bool {
        self.as_ref().is_response(payload_type)
    }

    /// Construct an alive check request message
    #[must_use]
    pub fn alive_check_request(protocol_version: ProtocolVersion) -> OwnedMessage {
        Message::alive_check_request(protocol_version).to_owned_message()
    }

    /// Construct an alive check response message
    #[must_use]
    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
    ) -> OwnedMessage {
        Message::alive_check_response(protocol_version, source_address).to_owned_message()
    }

    /// Construct a routing activation request message
    #[must_use]
    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> OwnedMessage {
        Message::routing_activation_request(
            protocol_version,
            source_address,
            activation_type,
            reserved_vehicle_manufacturer,
        )
        .to_owned_message()
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
    ) -> OwnedMessage {
        Message::routing_activation_response(
            protocol_version,
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        )
        .to_owned_message()
    }

    /// Construct a diagnostic message carrying owned user data
    ///
    /// # Panics
    /// Panics if `message.as_ref().encoded_size()` errors, or if the resulting
    /// size does not fit in a `u32`. Neither is reachable here: `encoded_size` is
    /// pure arithmetic over the struct's own fields (no I/O to fail), and its
    /// result is `4 + user_data.len()` (the two 2-byte addresses plus the user
    /// data; the 8-byte header is deliberately excluded, since this value becomes
    /// the header's payload length), far below `u32::MAX` for any `user_data`
    /// vector that can exist in memory.
    #[must_use]
    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        user_data: alloc::vec::Vec<u8>,
    ) -> OwnedMessage {
        let message = OwnedDiagnosticMessage {
            source_address,
            target_address,
            user_data,
        };
        let payload_size = payload_len(&message.as_ref());
        OwnedMessage {
            header: Header::new(
                protocol_version,
                PayloadType::DiagnosticMessage,
                payload_size,
            ),
            payload: OwnedPayload::DiagnosticMessage(message),
        }
    }

    /// Construct a diagnostic message acknowledgement carrying owned data
    ///
    /// # Known limitation
    /// The header is stamped with `PayloadType::DiagnosticMessagePositiveAcknowledge`
    /// (0x8002) regardless of `ack_code`, so a negative
    /// [`DiagnosticAckCode`] is currently emitted under the positive payload type
    /// instead of `DiagnosticMessageNegativeAcknowledge` (0x8003). This is a known
    /// open issue deferred to a follow-up change.
    ///
    /// # Panics
    /// Panics if `ack.as_ref().encoded_size()` errors, or if the resulting size
    /// does not fit in a `u32`. Neither is reachable here: `encoded_size` is pure
    /// arithmetic over the struct's own fields (no I/O to fail), and its result is
    /// far below `u32::MAX` for any `previous_message_data` vector that can exist
    /// in memory.
    #[must_use]
    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        ack_code: DiagnosticAckCode,
        previous_message_data: alloc::vec::Vec<u8>,
    ) -> OwnedMessage {
        let ack = OwnedDiagnosticMessageAck {
            source_address,
            target_address,
            ack_code,
            previous_message_data,
        };
        let payload_size = payload_len(&ack.as_ref());
        OwnedMessage {
            header: Header::new(
                protocol_version,
                PayloadType::DiagnosticMessagePositiveAcknowledge,
                payload_size,
            ),
            payload: OwnedPayload::DiagnosticMessageAck(ack),
        }
    }
}

/// Encode delegates through the borrowed view so there is exactly one wire
/// implementation (`Message<'_>: Encode`). This is the impl `message_codec.rs`'s
/// `Encoder<&OwnedMessage>` relies on (pass 4 risk item 3).
#[cfg(feature = "alloc")]
impl Encode for OwnedMessage {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        self.as_ref().encoded_size()
    }

    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        self.as_ref().encode(writer)
    }
}

/// Create a default diagnostic message (used as a placeholder when waiting for any
/// response — `client_inner.rs`). `Message<'a>` deliberately has no `Default`; the
/// placeholder needs owned (empty) data.
#[cfg(feature = "alloc")]
impl Default for OwnedMessage {
    fn default() -> Self {
        OwnedMessage {
            header: Header::new(ProtocolVersion::V2012, PayloadType::DiagnosticMessage, 0),
            payload: OwnedPayload::DiagnosticMessage(OwnedDiagnosticMessage {
                source_address: LogicalAddress(0),
                target_address: LogicalAddress(0),
                user_data: alloc::vec::Vec::new(),
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
        let deserialized_message: Message<'_> = Message::decode(&buf).unwrap().0;
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 1);
        let buf: [u8; 15] = [
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message: Message<'_> = Message::decode(&buf).unwrap().0;
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
            Message::decode(&buf),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }

    /// Encode a message into a stack buffer and frame it back out again, without any
    /// `Vec`/`alloc` involved. Exercises the `no_std` / `no_alloc` API surface.
    #[test]
    fn test_no_std_stack_buffer_roundtrip() {
        let message: Message<'_> = Message::diagnostic_message(
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

        let (frame, consumed) = crate::try_frame(&buf[..written]).unwrap().unwrap();
        assert_eq!(consumed, written);
        let payload = Payload::decode(frame.payload, frame.header.payload_type).unwrap();
        let decoded = Message {
            header: frame.header,
            payload,
        };
        assert_eq!(decoded, message);
    }

    /// A routing activation response carrying `oem_specific` must set the header payload
    /// length to match the 13 bytes `encode` writes, so the framed bytes round-trip.
    /// (Regression test: the constructor previously hardcoded a length of 9.)
    #[test]
    fn test_routing_activation_response_oem_specific_round_trip() {
        let message: Message<'_> = Message::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        // 9 fixed bytes + 4 oem-specific bytes.
        assert_eq!(message.header.payload_length, 13);

        let mut buf = [0u8; 64];
        let written = {
            let mut writer: &mut [u8] = &mut buf;
            message.encode(&mut writer).unwrap()
        };

        let (frame, consumed) = crate::try_frame(&buf[..written]).unwrap().unwrap();
        assert_eq!(consumed, written);
        let payload = Payload::decode(frame.payload, frame.header.payload_type).unwrap();
        let decoded = Message {
            header: frame.header,
            payload,
        };
        assert_eq!(decoded, message);
    }
}

/// Gate A follow-up: lock the borrowed<->owned conversions (correct today only by
/// inspection) and the owned/borrowed constructor header agreement as permanent
/// regressions. Alloc-gated because `to_owned_payload`/`to_owned_message` and the
/// `Owned*` constructors only exist under `feature = "alloc"`.
#[cfg(all(test, feature = "alloc"))]
mod alloc_conversion_tests {
    use super::*;
    use crate::messages::{
        AliveCheckResponse, DiagnosticAckCode, DiagnosticMessage, DiagnosticMessageAck,
        DiagnosticPowerModeCode, EntityStatusNodeType, EntityStatusResponse, FurtherActionRequired,
        NackCode, RoutingActivationResponseCode, VehicleIdentificationResponse, VinGidSyncStatus,
    };
    use alloc::vec::Vec;

    /// Construct one representative value of every `Payload<'_>` variant and assert that
    /// round-tripping through `to_owned_payload` / `OwnedPayload::as_ref` reproduces the
    /// original exactly. A future variant that is dropped from, or mis-mapped in, either
    /// conversion match will fail this test.
    #[test]
    fn payload_conversion_roundtrip_all_variants() {
        let diag_data = [0x10u8, 0x02];
        let ack_data = [0x3Eu8, 0x00, 0xAA];

        let values: Vec<Payload<'_>> = alloc::vec![
            Payload::DoIPNack(NackCode::IncorrectPatternFormat),
            Payload::AliveCheckRequest,
            Payload::AliveCheckResponse(AliveCheckResponse {
                source_address: LogicalAddress(0x0E00),
            }),
            Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: LogicalAddress(0x0E00),
                target_address: LogicalAddress(0x1000),
                user_data: &diag_data[..],
            }),
            Payload::DiagnosticMessageAck(DiagnosticMessageAck {
                source_address: LogicalAddress(0xFFFF),
                target_address: LogicalAddress(0x0001),
                ack_code: DiagnosticAckCode::TransportProtocolError,
                previous_message_data: &ack_data[..],
            }),
            Payload::DiagnosticMessageNack,
            Payload::EntityStatusRequest,
            Payload::EntityStatusResponse(EntityStatusResponse {
                node_type: EntityStatusNodeType::DoIPGateway,
                max_concurrent_tcp_sockets: 4,
                open_tcp_sockets: 0,
                max_data_size: 0x0000_FFFF,
            }),
            Payload::PowerModeInfoResponse(DiagnosticPowerModeCode::Ready),
            Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address: LogicalAddress(0x0E00),
                activation_type: ActivationTypeCode::Default,
                reserved: [0, 0, 0, 0],
                reserved_vehicle_manufacturer: Some([0xDE, 0xAD, 0xBE, 0xEF]),
            }),
            Payload::RoutingActivationResponse(RoutingActivationResponse {
                logical_address_tester: LogicalAddress(0x0E00),
                logical_address_of_doip_entity: LogicalAddress(0x1000),
                routing_activation_response_code:
                    RoutingActivationResponseCode::RoutingSuccessfullyActivated,
                reserved_oem: [0, 0, 0, 0],
                oem_specific: Some([0xDE, 0xAD, 0xBE, 0xEF]),
            }),
            Payload::VehicleAnnouncement(VehicleIdentificationResponse {
                vin: [0x41; 17],
                logical_address: LogicalAddress(0x0E00),
                entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
                group_id: Some([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }),
            Payload::VehicleIdentificationRequest,
            Payload::VehicleIdentificationResponse(VehicleIdentificationResponse {
                vin: *b"1HGCM82633A004352",
                logical_address: LogicalAddress(0x1000),
                entity_id: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
                group_id: None,
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }),
        ];

        assert_eq!(values.len(), 14, "expected one value per Payload variant");

        for p in &values {
            let owned = p.to_owned_payload();
            assert_eq!(&owned.as_ref(), p, "conversion mismatch for {p:?}");
        }
    }

    /// Owned constructors must produce a header identical to their borrowed
    /// counterpart's header for the same arguments, including the `payload_length`
    /// arithmetic (locks the routing-activation length-arithmetic regression class).
    #[test]
    fn owned_message_matches_borrowed_headers() {
        // routing_activation_response WITH oem_specific = Some(_) (13-byte case).
        let borrowed = Message::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        let owned = OwnedMessage::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        assert_eq!(borrowed.header, owned.header);
        assert_eq!(borrowed.header.payload_length, 13);

        // diagnostic_message with non-empty data.
        let data = [0x22u8, 0xF1, 0x90, 0x00];
        let borrowed = Message::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0xE400),
            LogicalAddress(0x00FF),
            &data[..],
        );
        let owned = OwnedMessage::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0xE400),
            LogicalAddress(0x00FF),
            data.to_vec(),
        );
        assert_eq!(borrowed.header, owned.header);
        assert_eq!(borrowed.to_owned_message().as_ref().header, borrowed.header);

        // routing_activation_request with a Some tail.
        let borrowed = Message::routing_activation_request(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            ActivationTypeCode::Default,
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        let owned = OwnedMessage::routing_activation_request(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            ActivationTypeCode::Default,
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        assert_eq!(borrowed.header, owned.header);
    }
}
