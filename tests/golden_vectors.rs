//! Golden wire-format vectors: encode representative values of every wire type and
//! compare byte-for-byte against fixtures captured from pre-migration `main`.
//!
//! Capture mode (run ONCE, before the migration starts, then commit `tests/golden/`):
//!   GOLDEN_WRITE=1 cargo test --test golden_vectors
//! Verify mode (every checkpoint from WP2 on):
//!   cargo test --test golden_vectors

use std::path::PathBuf;
use std::{env, fs};

use simple_doip::LogicalAddress;
use simple_doip::messages::{
    ActivationTypeCode, AliveCheckResponse, DiagnosticAckCode, DiagnosticMessage,
    DiagnosticMessageAck, DiagnosticPowerModeCode, Encode, EntityStatusNodeType,
    EntityStatusResponse, FurtherActionRequired, Header, NackCode, PayloadType, ProtocolVersion,
    RoutingActivationRequest, RoutingActivationResponse, RoutingActivationResponseCode,
    VehicleIdentificationResponse, VinGidSyncStatus,
};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_bytes(name: &str, bytes: &[u8]) {
    let path = golden_dir().join(format!("{name}.hex"));
    let actual = hex(bytes);
    if env::var_os("GOLDEN_WRITE").is_some() {
        fs::create_dir_all(golden_dir()).unwrap();
        fs::write(&path, format!("{actual}\n")).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{name}: missing fixture {}: {e}; capture with GOLDEN_WRITE=1 on pre-migration main",
            path.display()
        )
    });
    assert_eq!(actual, expected.trim(), "{name}: wire bytes changed");
}

// NOTE: encode errors are matched, not `.unwrap()`-ed, so this file compiles both
// before WP2 (local `Encode`, concrete `MessageError`) and after (L0 `Encode` with an
// associated `Error` type that this generic context cannot bound by `Debug` pre-WP2).
fn check(name: &str, value: &impl Encode) {
    let mut buf = [0u8; 128];
    let written = {
        let mut writer: &mut [u8] = &mut buf;
        match value.encode(&mut writer) {
            Ok(n) => n,
            Err(_) => panic!("{name}: encode failed"),
        }
    };
    check_bytes(name, &buf[..written]);
}

/// Full frame: 8-byte header (length derived by encoding the payload once) + payload body.
fn check_frame(
    name: &str,
    protocol_version: ProtocolVersion,
    payload_type: PayloadType,
    payload: &impl Encode,
) {
    // Size the payload by encoding it once — avoids naming encoded_size(), whose return
    // type changes across the migration.
    let mut payload_buf = [0u8; 128];
    let payload_len = {
        let mut writer: &mut [u8] = &mut payload_buf;
        match payload.encode(&mut writer) {
            Ok(n) => n,
            Err(_) => panic!("{name}: payload sizing encode failed"),
        }
    };
    let header = Header::new(
        protocol_version,
        payload_type,
        u32::try_from(payload_len).unwrap(),
    );
    let mut buf = [0u8; 128];
    let written = {
        let mut writer: &mut [u8] = &mut buf;
        let header_written = match header.encode(&mut writer) {
            Ok(n) => n,
            Err(_) => panic!("{name}: header encode failed"),
        };
        let payload_written = match payload.encode(&mut writer) {
            Ok(n) => n,
            Err(_) => panic!("{name}: payload encode failed"),
        };
        header_written + payload_written
    };
    check_bytes(name, &buf[..written]);
}

#[test]
fn golden_header() {
    check(
        "header_nack",
        &Header::new(ProtocolVersion::V2012, PayloadType::NegativeAcknowledge, 1),
    );
    check(
        "header_diag_9",
        &Header::new(ProtocolVersion::V2012, PayloadType::DiagnosticMessage, 9),
    );
    check(
        "header_vid_request_2010",
        &Header::new(
            ProtocolVersion::V2010,
            PayloadType::VehicleIdentificationRequest,
            0,
        ),
    );
}

#[test]
fn golden_nack_code() {
    check("nack_incorrect_pattern", &NackCode::IncorrectPatternFormat);
    check("nack_unknown_payload_type", &NackCode::UnknownPayloadType);
    check("nack_message_too_large", &NackCode::MessageTooLarge);
}

#[test]
fn golden_alive_check_response() {
    check(
        "alive_check_response_0000",
        &AliveCheckResponse {
            source_address: LogicalAddress(0x0000),
        },
    );
    check(
        "alive_check_response_0e00",
        &AliveCheckResponse {
            source_address: LogicalAddress(0x0E00),
        },
    );
    check(
        "alive_check_response_ffff",
        &AliveCheckResponse {
            source_address: LogicalAddress(0xFFFF),
        },
    );
}

#[test]
fn golden_power_mode_code() {
    check("power_mode_not_ready", &DiagnosticPowerModeCode::NotReady);
    check("power_mode_ready", &DiagnosticPowerModeCode::Ready);
    check(
        "power_mode_not_supported",
        &DiagnosticPowerModeCode::NotSupported,
    );
}

#[test]
fn golden_entity_status_response() {
    check(
        "entity_status_gateway",
        &EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPGateway,
            max_concurrent_tcp_sockets: 4,
            open_tcp_sockets: 0,
            max_data_size: 0x0000_FFFF,
        },
    );
    check(
        "entity_status_node",
        &EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: 1,
            open_tcp_sockets: 1,
            max_data_size: 64,
        },
    );
    check(
        "entity_status_max",
        &EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: 255,
            open_tcp_sockets: 255,
            max_data_size: u32::MAX,
        },
    );
}

#[test]
fn golden_diagnostic_message() {
    check(
        "diag_msg_empty",
        &DiagnosticMessage {
            source_address: LogicalAddress(0x0E00),
            target_address: LogicalAddress(0x1000),
            user_data: &[][..],
        },
    );
    check(
        "diag_msg_session_control",
        &DiagnosticMessage {
            source_address: LogicalAddress(0x0E00),
            target_address: LogicalAddress(0x1000),
            user_data: &[0x10u8, 0x02][..],
        },
    );
    check(
        "diag_msg_16_bytes",
        &DiagnosticMessage {
            source_address: LogicalAddress(0xE400),
            target_address: LogicalAddress(0x00FF),
            user_data: &[
                0x22u8, 0xF1, 0x90, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
                0x0A, 0x0B, 0x0C,
            ][..],
        },
    );
}

#[test]
fn golden_diagnostic_message_ack() {
    check(
        "diag_ack_positive_empty",
        &DiagnosticMessageAck {
            source_address: LogicalAddress(0x1000),
            target_address: LogicalAddress(0x0E00),
            ack_code: DiagnosticAckCode::RoutingConfirmationAck,
            previous_message_data: &[][..],
        },
    );
    check(
        "diag_ack_out_of_memory",
        &DiagnosticMessageAck {
            source_address: LogicalAddress(0x1000),
            target_address: LogicalAddress(0x0E00),
            ack_code: DiagnosticAckCode::OutOfMemory,
            previous_message_data: &[0x10u8, 0x02][..],
        },
    );
    check(
        "diag_ack_transport_error",
        &DiagnosticMessageAck {
            source_address: LogicalAddress(0xFFFF),
            target_address: LogicalAddress(0x0001),
            ack_code: DiagnosticAckCode::TransportProtocolError,
            previous_message_data: &[0x3Eu8, 0x00, 0xAA][..],
        },
    );
}

#[test]
fn golden_routing_activation_request() {
    check(
        "routing_req_default_no_tail",
        &RoutingActivationRequest {
            source_address: LogicalAddress(0x0E00),
            activation_type: ActivationTypeCode::Default,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer: None,
        },
    );
    check(
        "routing_req_default_with_tail",
        &RoutingActivationRequest {
            source_address: LogicalAddress(0x0E00),
            activation_type: ActivationTypeCode::Default,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer: Some([0xDE, 0xAD, 0xBE, 0xEF]),
        },
    );
    check(
        "routing_req_central_security",
        &RoutingActivationRequest {
            source_address: LogicalAddress(0xE400),
            activation_type: ActivationTypeCode::CentralSecurity,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer: None,
        },
    );
}

#[test]
fn golden_routing_activation_response() {
    check(
        "routing_resp_success_no_oem",
        &RoutingActivationResponse {
            logical_address_tester: LogicalAddress(0x0E00),
            logical_address_of_doip_entity: LogicalAddress(0x1000),
            routing_activation_response_code:
                RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            reserved_oem: [0, 0, 0, 0],
            oem_specific: None,
        },
    );
    check(
        "routing_resp_success_with_oem",
        &RoutingActivationResponse {
            logical_address_tester: LogicalAddress(0x0E00),
            logical_address_of_doip_entity: LogicalAddress(0x1000),
            routing_activation_response_code:
                RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            reserved_oem: [0, 0, 0, 0],
            oem_specific: Some([0xDE, 0xAD, 0xBE, 0xEF]),
        },
    );
    check(
        "routing_resp_denied_unknown_sa",
        &RoutingActivationResponse {
            logical_address_tester: LogicalAddress(0x0E00),
            logical_address_of_doip_entity: LogicalAddress(0x1000),
            routing_activation_response_code:
                RoutingActivationResponseCode::DeniedUnknownSourceAddress,
            reserved_oem: [0xAA, 0xBB, 0xCC, 0xDD],
            oem_specific: None,
        },
    );
}

#[test]
fn golden_vehicle_identification_response() {
    check(
        "vid_resp_with_group_id",
        &VehicleIdentificationResponse {
            vin: [0x41; 17],
            logical_address: LogicalAddress(0x0E00),
            entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            group_id: Some([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        },
    );
    check(
        "vid_resp_no_group_id",
        &VehicleIdentificationResponse {
            vin: *b"1HGCM82633A004352",
            logical_address: LogicalAddress(0x1000),
            entity_id: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            group_id: None,
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        },
    );
    check(
        "vid_resp_further_action",
        &VehicleIdentificationResponse {
            vin: [0x30; 17],
            logical_address: LogicalAddress(0xE400),
            entity_id: [0, 0, 0, 0, 0, 1],
            group_id: Some([1, 1, 1, 1, 1, 1]),
            further_action:
                FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity,
            vin_gid_sync_status: VinGidSyncStatus::Incomplete,
        },
    );
}

/// Full frames (header + payload as one buffer) — covers `Payload`/`Message` level
/// composition, including the header-length arithmetic the routing-activation
/// regression test in `src/messages/mod.rs` exists for.
#[test]
fn golden_full_frames() {
    check_frame(
        "frame_nack",
        ProtocolVersion::V2012,
        PayloadType::NegativeAcknowledge,
        &NackCode::IncorrectPatternFormat,
    );
    check_frame(
        "frame_diag_msg",
        ProtocolVersion::V2012,
        PayloadType::DiagnosticMessage,
        &DiagnosticMessage {
            source_address: LogicalAddress(0x0E00),
            target_address: LogicalAddress(0x1000),
            user_data: &[0x10u8, 0x02][..],
        },
    );
    check_frame(
        "frame_routing_resp_oem",
        ProtocolVersion::V2012,
        PayloadType::RoutingActivationResponse,
        &RoutingActivationResponse {
            logical_address_tester: LogicalAddress(0x0E00),
            logical_address_of_doip_entity: LogicalAddress(0x1000),
            routing_activation_response_code:
                RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            reserved_oem: [0, 0, 0, 0],
            oem_specific: Some([0xDE, 0xAD, 0xBE, 0xEF]),
        },
    );
}
