//! Property tests over the wire format: every byte-coded enum survives a byte
//! conversion round trip, and every message type survives an encode/decode
//! round trip, for arbitrary inputs rather than the handful of values a
//! table-driven test happens to name.
//!
//! These complement `golden_vectors.rs` rather than duplicating it. The golden
//! vectors pin the exact bytes the crate emits, so they catch a change to the
//! wire format; these check that `encode` and `decode` agree with each other
//! across the whole input space, which catches a field written in one order and
//! read in another. Neither finds a misreading of the standard that `encode`
//! and `decode` share symmetrically -- that is what the golden fixtures are for.
//!
//! The cases here were written by `@gavin-dunlap-luminar` in `simple_doip#1`
//! against the pre-`no_std` `write`/`read` API that 0.2.0 removed. The
//! properties are his; the calls are rewritten for `Encode`/`Decode`.

use proptest::prelude::*;
use simple_doip::{
    LogicalAddress,
    messages::{
        ActivationTypeCode, AliveCheckResponse, Decode, DiagnosticAckCode, DiagnosticMessage,
        DiagnosticMessageAck, DiagnosticPowerModeCode, Encode, EntityStatusNodeType,
        EntityStatusResponse, FurtherActionRequired, Header, Message, MessageError, NackCode,
        Payload, PayloadType, ProtocolVersion, RoutingActivationRequest, RoutingActivationResponse,
        RoutingActivationResponseCode, VehicleIdentificationResponse, VinGidSyncStatus,
    },
};

/// Encode into a fixed buffer the way a bare-metal caller would -- a `&mut [u8]`
/// is the `embedded_io::Write` this crate is built around -- and return the
/// written prefix. Also asserts `encoded_size()` agrees with what `encode`
/// wrote, since a closed-form size override that drifts from its `encode`
/// corrupts the header's `payload_length` silently.
fn encode_to<'buf>(value: &impl Encode<Error = MessageError>, buf: &'buf mut [u8]) -> &'buf [u8] {
    let written = {
        let mut writer: &mut [u8] = buf;
        value.encode(&mut writer).expect("encode failed")
    };
    assert_eq!(
        value.encoded_size().expect("encoded_size failed"),
        written,
        "encoded_size() disagrees with encode()"
    );
    &buf[..written]
}

/// `byte -> enum -> byte` for every enum whose wire form is a single byte.
/// A variant that discards the byte it did not recognise fails here.
macro_rules! byte_code_roundtrip {
    ($name:ident, $ty:ty) => {
        proptest! {
            #[test]
            fn $name(byte in any::<u8>()) {
                let decoded = <$ty>::from(byte);
                let back: u8 = decoded.into();
                prop_assert_eq!(byte, back);
            }
        }
    };
}

byte_code_roundtrip!(prop_protocol_version_roundtrip, ProtocolVersion);
byte_code_roundtrip!(prop_activation_type_roundtrip, ActivationTypeCode);
byte_code_roundtrip!(prop_diagnostic_ack_code_roundtrip, DiagnosticAckCode);
byte_code_roundtrip!(prop_entity_node_type_roundtrip, EntityStatusNodeType);
byte_code_roundtrip!(prop_further_action_roundtrip, FurtherActionRequired);
byte_code_roundtrip!(prop_nack_code_roundtrip, NackCode);
byte_code_roundtrip!(prop_power_mode_code_roundtrip, DiagnosticPowerModeCode);
byte_code_roundtrip!(
    prop_routing_response_code_roundtrip,
    RoutingActivationResponseCode
);
byte_code_roundtrip!(prop_vin_gid_sync_roundtrip, VinGidSyncStatus);

proptest! {
    /// `PayloadType` is the one two-byte code.
    #[test]
    fn prop_payload_type_roundtrip(value in any::<u16>()) {
        let decoded = PayloadType::from(value);
        let back: u16 = decoded.into();
        prop_assert_eq!(value, back);
    }

    /// ISO 13400-2 reserves the tester address range `0x0E00..=0x0FFF`.
    #[test]
    fn prop_logical_address_client_range(addr in 0x0E00u16..=0x0FFF) {
        prop_assert!(LogicalAddress(addr).is_valid_client_address());
    }

    /// Everything outside that range is not a tester address.
    #[test]
    fn prop_logical_address_outside_client_range(
        addr in prop_oneof![0x0000u16..0x0E00, 0x1000u16..=0xFFFF],
    ) {
        prop_assert!(!LogicalAddress(addr).is_valid_client_address());
    }

    #[test]
    fn prop_header_roundtrip(
        version in any::<u8>().prop_map(ProtocolVersion::from),
        payload_type in any::<u16>().prop_map(PayloadType::from),
        payload_length in any::<u32>(),
    ) {
        let header = Header::new(version, payload_type, payload_length);
        let mut buf = [0u8; Header::SIZE];
        let bytes = encode_to(&header, &mut buf);
        prop_assert_eq!(bytes.len(), Header::SIZE);

        let parsed = Header::decode_exact(bytes).expect("header decode failed");
        prop_assert_eq!(header, parsed);
    }

    #[test]
    fn prop_alive_check_response_roundtrip(source in any::<u16>()) {
        let value = AliveCheckResponse { source_address: LogicalAddress(source) };
        let mut buf = [0u8; 16];
        let bytes = encode_to(&value, &mut buf);
        let parsed = AliveCheckResponse::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    #[test]
    fn prop_diagnostic_message_roundtrip(
        source in any::<u16>(),
        target in any::<u16>(),
        user_data in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let value = DiagnosticMessage {
            source_address: LogicalAddress(source),
            target_address: LogicalAddress(target),
            user_data: &user_data,
        };
        let mut buf = [0u8; 128];
        let bytes = encode_to(&value, &mut buf);
        let parsed = DiagnosticMessage::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    #[test]
    fn prop_diagnostic_message_ack_roundtrip(
        source in any::<u16>(),
        target in any::<u16>(),
        ack_code in any::<u8>().prop_map(DiagnosticAckCode::from),
        previous in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let value = DiagnosticMessageAck {
            source_address: LogicalAddress(source),
            target_address: LogicalAddress(target),
            ack_code,
            previous_message_data: &previous,
        };
        let mut buf = [0u8; 128];
        let bytes = encode_to(&value, &mut buf);
        let parsed = DiagnosticMessageAck::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    #[test]
    fn prop_entity_status_response_roundtrip(
        node_type in any::<u8>().prop_map(EntityStatusNodeType::from),
        max_concurrent_tcp_sockets in any::<u8>(),
        open_tcp_sockets in any::<u8>(),
        max_data_size in any::<u32>(),
    ) {
        let value = EntityStatusResponse {
            node_type,
            max_concurrent_tcp_sockets,
            open_tcp_sockets,
            max_data_size,
        };
        let mut buf = [0u8; 32];
        let bytes = encode_to(&value, &mut buf);
        let parsed = EntityStatusResponse::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    /// Both shapes of the request: with and without the optional
    /// vehicle-manufacturer field, which is the length difference the decoder
    /// has to infer.
    #[test]
    fn prop_routing_activation_request_roundtrip(
        source in any::<u16>(),
        activation_type in any::<u8>().prop_map(ActivationTypeCode::from),
        reserved in any::<[u8; 4]>(),
        vehicle_manufacturer in proptest::option::of(any::<[u8; 4]>()),
    ) {
        let value = RoutingActivationRequest {
            source_address: LogicalAddress(source),
            activation_type,
            reserved,
            reserved_vehicle_manufacturer: vehicle_manufacturer,
        };
        let mut buf = [0u8; 32];
        let bytes = encode_to(&value, &mut buf);
        let parsed = RoutingActivationRequest::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    #[test]
    fn prop_routing_activation_response_roundtrip(
        tester in any::<u16>(),
        entity in any::<u16>(),
        code in any::<u8>().prop_map(RoutingActivationResponseCode::from),
        reserved_oem in any::<[u8; 4]>(),
        oem_specific in proptest::option::of(any::<[u8; 4]>()),
    ) {
        let value = RoutingActivationResponse {
            logical_address_tester: LogicalAddress(tester),
            logical_address_of_doip_entity: LogicalAddress(entity),
            routing_activation_response_code: code,
            reserved_oem,
            oem_specific,
        };
        let mut buf = [0u8; 32];
        let bytes = encode_to(&value, &mut buf);
        let parsed = RoutingActivationResponse::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    /// `group_id` is optional on the wire, so both shapes go through the same
    /// property rather than needing a test each.
    #[test]
    fn prop_vehicle_identification_response_roundtrip(
        vin in any::<[u8; 17]>(),
        logical_address in any::<u16>(),
        entity_id in any::<[u8; 6]>(),
        group_id in proptest::option::of(any::<[u8; 6]>()),
        further_action in any::<u8>().prop_map(FurtherActionRequired::from),
        vin_gid_sync_status in any::<u8>().prop_map(VinGidSyncStatus::from),
    ) {
        let value = VehicleIdentificationResponse {
            vin,
            logical_address: LogicalAddress(logical_address),
            entity_id,
            group_id,
            further_action,
            vin_gid_sync_status,
        };
        let mut buf = [0u8; 64];
        let bytes = encode_to(&value, &mut buf);
        let parsed = VehicleIdentificationResponse::decode_exact(bytes).expect("decode failed");
        prop_assert_eq!(value, parsed);
    }

    /// A whole frame -- header plus body -- through `Message`, which is what a
    /// peer actually puts on the wire.
    #[test]
    fn prop_full_nack_frame_roundtrip(code in any::<u8>().prop_map(NackCode::from)) {
        let payload = Payload::DoIPNack(code);
        let mut buf = [0u8; 32];
        let bytes = frame(ProtocolVersion::V2012, PayloadType::NegativeAcknowledge, &payload, &mut buf);

        let (parsed, rest) = Message::decode(bytes).expect("message decode failed");
        prop_assert!(rest.is_empty());
        prop_assert_eq!(parsed.header.payload_type, PayloadType::NegativeAcknowledge);
        prop_assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn prop_full_diagnostic_frame_roundtrip(
        source in any::<u16>(),
        target in any::<u16>(),
        user_data in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let payload = Payload::DiagnosticMessage(DiagnosticMessage {
            source_address: LogicalAddress(source),
            target_address: LogicalAddress(target),
            user_data: &user_data,
        });
        let mut buf = [0u8; 128];
        let bytes = frame(ProtocolVersion::V2012, PayloadType::DiagnosticMessage, &payload, &mut buf);

        let (parsed, rest) = Message::decode(bytes).expect("message decode failed");
        prop_assert!(rest.is_empty());
        prop_assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn prop_full_alive_check_frame_roundtrip(source in any::<u16>()) {
        let payload = Payload::AliveCheckResponse(AliveCheckResponse {
            source_address: LogicalAddress(source),
        });
        let mut buf = [0u8; 32];
        let bytes = frame(ProtocolVersion::V2012, PayloadType::AliveCheckResponse, &payload, &mut buf);

        let (parsed, rest) = Message::decode(bytes).expect("message decode failed");
        prop_assert!(rest.is_empty());
        prop_assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn prop_full_routing_activation_request_frame_roundtrip(
        source in any::<u16>(),
        activation_type in any::<u8>().prop_map(ActivationTypeCode::from),
        reserved in any::<[u8; 4]>(),
        vehicle_manufacturer in proptest::option::of(any::<[u8; 4]>()),
    ) {
        let payload = Payload::RoutingActivationRequest(RoutingActivationRequest {
            source_address: LogicalAddress(source),
            activation_type,
            reserved,
            reserved_vehicle_manufacturer: vehicle_manufacturer,
        });
        let mut buf = [0u8; 64];
        let bytes = frame(
            ProtocolVersion::V2012,
            PayloadType::RoutingActivationRequest,
            &payload,
            &mut buf,
        );

        let (parsed, rest) = Message::decode(bytes).expect("message decode failed");
        prop_assert!(rest.is_empty());
        prop_assert_eq!(parsed.payload, payload);
    }
}

/// Header (with `payload_length` taken from the body's `encoded_size`) followed
/// by the body, which is the frame layout ISO 13400-2 specifies.
fn frame<'buf>(
    protocol_version: ProtocolVersion,
    payload_type: PayloadType,
    payload: &Payload<'_>,
    buf: &'buf mut [u8],
) -> &'buf [u8] {
    let payload_len = payload.encoded_size().expect("payload encoded_size failed");
    let header = Header::new(
        protocol_version,
        payload_type,
        u32::try_from(payload_len).expect("payload fits in u32"),
    );
    let written = {
        let mut writer: &mut [u8] = buf;
        header.encode(&mut writer).expect("header encode failed")
            + payload.encode(&mut writer).expect("payload encode failed")
    };
    assert_eq!(written, Header::SIZE + payload_len, "frame size disagrees");
    &buf[..written]
}
