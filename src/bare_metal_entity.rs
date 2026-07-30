//! Bare-metal `DoIP` entity (server) for `no_std` targets.
//!
//! A sans-io ISO 13400-2 entity for platforms with a single diagnostic TCP
//! socket and no allocator, executor, or OS: the platform feeds raw bytes in
//! ([`Entity::on_udp_rx`], [`Entity::on_tcp_rx`]) and provides transmit /
//! dispatch callbacks ([`Callbacks`]); the entity owns all protocol state and
//! buffers, so one `static` [`Entity`] is the whole integration and the
//! platform keeps control of link placement.
//!
//! Behavior:
//!
//! - **UDP 13400** — answers vehicle identification requests (all three
//!   variants) with a vehicle announcement, and entity status requests with
//!   the single-socket status; malformed datagrams get a `DoIP` NACK.
//! - **TCP 13400** — reassembles the byte stream, performs routing activation
//!   (one tester at a time), acknowledges diagnostic messages, and forwards
//!   their payload to the [`Callbacks::on_uds_request`] dispatch; a positive
//!   response length is framed and sent back as a diagnostic message.
//! - [`TcpVerdict::Close`] tells the platform the connection is protocol-dead
//!   (ISO 13400-2 close actions); the platform must close the socket and call
//!   [`Entity::on_tcp_disconnect`].
//!
//! All methods must be called from a single context (no internal locking).

use crate::messages::{
    ActivationTypeCode, Decode, DiagnosticAckCode, DiagnosticMessage, Encode, EntityStatusNodeType,
    EntityStatusResponse, FurtherActionRequired, Header, Message, NackCode, Payload, PayloadType,
    ProtocolVersion, RoutingActivationRequest, VehicleIdentificationResponse, VinGidSyncStatus,
};
use crate::{LogicalAddress, try_frame};

/// Version stamped into every response header. V2012 (0x02) is the version
/// deployed diagnostic testers negotiate by default.
const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V2012;

/// VIN length fixed by ISO 13400-2.
pub const VIN_LEN: usize = 17;
/// Entity ID (EID) length fixed by ISO 13400-2.
pub const EID_LEN: usize = 6;
/// Group ID (GID) length fixed by ISO 13400-2.
pub const GID_LEN: usize = 6;

/// Largest UDS response the [`Callbacks::on_uds_request`] dispatch may
/// produce.
pub const UDS_RESP_CAP: usize = 1024;

/// SA + TA prefix of a diagnostic-message payload.
const DIAG_ADDR_LEN: usize = 4;

/// TCP reassembly capacity; bounds the largest inbound `DoIP` frame.
pub const TCP_RX_CAP: usize = 2048;

/// Largest inbound payload that can ever complete reassembly.
pub const MAX_RX_PAYLOAD: usize = TCP_RX_CAP - Header::SIZE;

/// TX staging capacity: fits the largest response (a diagnostic message
/// carrying a full UDS response).
const TX_CAP: usize = Header::SIZE + DIAG_ADDR_LEN + UDS_RESP_CAP;

/// Identity and addressing the entity announces and answers with.
#[derive(Clone, Copy, Debug)]
pub struct EntityConfig {
    /// Vehicle identification number, announced verbatim.
    pub vin: [u8; VIN_LEN],
    /// Entity ID (typically the MAC address).
    pub eid: [u8; EID_LEN],
    /// Group ID; all-zero means "not set" and is announced as absent.
    pub gid: [u8; GID_LEN],
    /// This entity's `DoIP` logical address.
    pub logical_address: u16,
}

/// Platform callbacks the entity drives its I/O and dispatch through.
///
/// Plain function pointers so the struct stays `Copy` and const-friendly;
/// platform state, if any, must live behind the functions.
#[derive(Clone, Copy, Debug)]
pub struct Callbacks {
    /// Send one UDP datagram to `dst_addr:dst_port` (address in host byte
    /// order) from the `DoIP` UDP socket. Returns `< 0` on failure.
    pub send_udp: fn(dst_addr: u32, dst_port: u16, data: &[u8]) -> i32,
    /// Send bytes on the diagnostic TCP connection. Returns `< 0` on failure.
    pub send_tcp: fn(data: &[u8]) -> i32,
    /// Handle one UDS request. A response written to `response_out` is
    /// reported by returning its length (`> 0`); `<= 0` means no response.
    pub on_uds_request: fn(request: &[u8], response_out: &mut [u8]) -> i32,
}

/// What the platform must do with the TCP connection after a byte feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum TcpVerdict {
    /// The connection is healthy; keep feeding bytes.
    KeepOpen,
    /// Protocol/framing state is unrecoverable — close the connection and
    /// call [`Entity::on_tcp_disconnect`].
    Close,
}

/// The `DoIP` entity: identity, session state, and all buffers.
///
/// `const`-constructible so the platform can declare it as a `static` in a
/// linker section of its choosing.
pub struct Entity {
    vin: [u8; VIN_LEN],
    eid: [u8; EID_LEN],
    gid: [u8; GID_LEN],
    logical_address: u16,
    /// `Some` between [`Entity::init`] and [`Entity::deinit`].
    callbacks: Option<Callbacks>,
    /// Source address registered by a successful routing activation.
    active_tester: Option<u16>,
    rx_len: usize,
    rx_buf: [u8; TCP_RX_CAP],
    tx_buf: [u8; TX_CAP],
    uds_resp_buf: [u8; UDS_RESP_CAP],
}

impl core::fmt::Debug for Entity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Entity")
            .field("logical_address", &self.logical_address)
            .field("initialised", &self.callbacks.is_some())
            .field("active_tester", &self.active_tester)
            .field("rx_len", &self.rx_len)
            .finish_non_exhaustive()
    }
}

impl Default for Entity {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity {
    /// An uninitialised entity; every input is ignored until [`Entity::init`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            vin: [0; VIN_LEN],
            eid: [0; EID_LEN],
            gid: [0; GID_LEN],
            logical_address: 0,
            callbacks: None,
            active_tester: None,
            rx_len: 0,
            rx_buf: [0; TCP_RX_CAP],
            tx_buf: [0; TX_CAP],
            uds_resp_buf: [0; UDS_RESP_CAP],
        }
    }

    /// Adopt an identity and platform callbacks and start serving. Resets any
    /// previous session state.
    pub fn init(&mut self, config: &EntityConfig, callbacks: Callbacks) {
        self.vin = config.vin;
        self.eid = config.eid;
        self.gid = config.gid;
        self.logical_address = config.logical_address;
        self.callbacks = Some(callbacks);
        self.on_tcp_disconnect();
    }

    /// Stop serving; every input is ignored until the next [`Entity::init`].
    pub fn deinit(&mut self) {
        self.callbacks = None;
        self.on_tcp_disconnect();
    }

    /// One UDP datagram from the `DoIP` discovery socket. `src_addr` in host
    /// byte order.
    pub fn on_udp_rx(&mut self, src_addr: u32, src_port: u16, data: &[u8]) {
        let Some(cb) = self.callbacks else { return };
        if data.is_empty() {
            return;
        }

        let frame = match try_frame(data) {
            Ok(Some((frame, _consumed))) => frame,
            // A datagram must carry one complete message; a short one has no
            // valid reply semantics worth a NACK.
            Ok(None) => return,
            Err(_) => {
                if let Some(msg) = framed(
                    PayloadType::NegativeAcknowledge,
                    Payload::DoIPNack(NackCode::IncorrectPatternFormat),
                ) {
                    send_udp_msg(&cb, &mut self.tx_buf, src_addr, src_port, &msg);
                }
                return;
            }
        };

        let reply = match Payload::decode(frame.payload, frame.header.payload_type) {
            // Payload::decode collapses all three VIR variants (plain,
            // with-EID, with-VIN) into this unit variant, so EID/VIN filters
            // cannot be checked and every variant gets the announcement.
            Ok(Payload::VehicleIdentificationRequest) => self.vehicle_announcement(),
            Ok(Payload::EntityStatusRequest) => self.entity_status(),
            Ok(_) => None,
            Err(_) => framed(
                PayloadType::NegativeAcknowledge,
                Payload::DoIPNack(NackCode::UnknownPayloadType),
            ),
        };
        if let Some(msg) = reply {
            send_udp_msg(&cb, &mut self.tx_buf, src_addr, src_port, &msg);
        }
    }

    /// A chunk of the diagnostic TCP stream (any framing).
    pub fn on_tcp_rx(&mut self, data: &[u8]) -> TcpVerdict {
        let Some(cb) = self.callbacks else {
            return TcpVerdict::Close;
        };
        if data.is_empty() {
            return TcpVerdict::KeepOpen;
        }
        let mut session = Session {
            cb,
            own_address: self.logical_address,
            active_tester: &mut self.active_tester,
            tx_buf: &mut self.tx_buf,
            uds_resp_buf: &mut self.uds_resp_buf,
        };
        handle_tcp_bytes(&mut session, &mut self.rx_len, &mut self.rx_buf, data)
    }

    /// The diagnostic TCP connection went away (close, abort, error).
    pub fn on_tcp_disconnect(&mut self) {
        self.active_tester = None;
        self.rx_len = 0;
    }

    fn vehicle_announcement(&self) -> Option<Message<'static>> {
        let group_id = if self.gid == [0; GID_LEN] {
            None
        } else {
            Some(self.gid)
        };
        let response = VehicleIdentificationResponse {
            vin: self.vin,
            logical_address: LogicalAddress(self.logical_address),
            entity_id: self.eid,
            group_id,
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        };
        framed(
            PayloadType::VehicleAnnouncement,
            Payload::VehicleAnnouncement(response),
        )
    }

    fn entity_status(&self) -> Option<Message<'static>> {
        let response = EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: 1,
            open_tcp_sockets: u8::from(self.active_tester.is_some()),
            max_data_size: u32::try_from(MAX_RX_PAYLOAD).unwrap_or(u32::MAX),
        };
        framed(
            PayloadType::DoIPEntityStatusResponse,
            Payload::EntityStatusResponse(response),
        )
    }
}

/// Everything a TCP handler needs except the reassembly stream (which frames
/// borrow from, so it travels separately).
struct Session<'a> {
    cb: Callbacks,
    own_address: u16,
    active_tester: &'a mut Option<u16>,
    tx_buf: &'a mut [u8; TX_CAP],
    uds_resp_buf: &'a mut [u8; UDS_RESP_CAP],
}

/// Wrap `payload` in a header of the matching type. `None` only if the
/// payload cannot be sized, which no payload built here can trigger.
fn framed(payload_type: PayloadType, payload: Payload<'_>) -> Option<Message<'_>> {
    let size = u32::try_from(payload.encoded_size().ok()?).ok()?;
    Some(Message {
        header: Header::new(PROTOCOL_VERSION, payload_type, size),
        payload,
    })
}

fn encode_into(buf: &mut [u8], msg: &Message<'_>) -> Option<usize> {
    let mut writer: &mut [u8] = buf;
    msg.encode(&mut writer).ok()
}

fn send_udp_msg(
    cb: &Callbacks,
    tx: &mut [u8; TX_CAP],
    dst_addr: u32,
    dst_port: u16,
    msg: &Message<'_>,
) {
    if let Some(len) = encode_into(tx, msg) {
        (cb.send_udp)(dst_addr, dst_port, &tx[..len]);
    }
}

fn send_tcp_msg(cb: &Callbacks, tx: &mut [u8; TX_CAP], msg: &Message<'_>) {
    if let Some(len) = encode_into(tx, msg) {
        (cb.send_tcp)(&tx[..len]);
    }
}

fn send_tcp_nack(cb: &Callbacks, tx: &mut [u8; TX_CAP], code: NackCode) {
    if let Some(msg) = framed(PayloadType::NegativeAcknowledge, Payload::DoIPNack(code)) {
        send_tcp_msg(cb, tx, &msg);
    }
}

fn send_diag_ack(
    s: &mut Session<'_>,
    payload_type: PayloadType,
    tester: u16,
    code: DiagnosticAckCode,
) {
    let ack = crate::messages::DiagnosticMessageAck {
        source_address: LogicalAddress(s.own_address),
        target_address: LogicalAddress(tester),
        ack_code: code,
        previous_message_data: &[],
    };
    if let Some(msg) = framed(payload_type, Payload::DiagnosticMessageAck(ack)) {
        send_tcp_msg(&s.cb, s.tx_buf, &msg);
    }
}

fn handle_routing_activation(s: &mut Session<'_>, req: &RoutingActivationRequest) -> TcpVerdict {
    let source = req.source_address;
    let (code, verdict) = if !source.is_valid_client_address() {
        (
            crate::messages::RoutingActivationResponseCode::DeniedUnknownSourceAddress,
            TcpVerdict::Close,
        )
    } else if !matches!(
        req.activation_type,
        ActivationTypeCode::Default | ActivationTypeCode::RegulationRequired
    ) {
        (
            crate::messages::RoutingActivationResponseCode::DeniedUnsupportedRoutingActivationType,
            TcpVerdict::Close,
        )
    } else if s.active_tester.is_some_and(|active| active != source.0) {
        (
            crate::messages::RoutingActivationResponseCode::DeniedSourceAddressAlreadyActivated,
            TcpVerdict::Close,
        )
    } else {
        *s.active_tester = Some(source.0);
        (
            crate::messages::RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            TcpVerdict::KeepOpen,
        )
    };

    let msg = Message::routing_activation_response(
        PROTOCOL_VERSION,
        source,
        LogicalAddress(s.own_address),
        code,
        [0; 4],
        None,
    );
    send_tcp_msg(&s.cb, s.tx_buf, &msg);
    verdict
}

fn handle_diagnostic_message(s: &mut Session<'_>, dm: &DiagnosticMessage<'_>) -> TcpVerdict {
    let Some(tester) = *s.active_tester else {
        // ISO 13400-2: a diagnostic message on a socket without routing
        // activation invalidates the connection.
        send_diag_ack(
            s,
            PayloadType::DiagnosticMessageNegativeAcknowledge,
            dm.source_address.0,
            DiagnosticAckCode::InvalidSourceAddress,
        );
        return TcpVerdict::Close;
    };
    if dm.source_address.0 != tester {
        send_diag_ack(
            s,
            PayloadType::DiagnosticMessageNegativeAcknowledge,
            tester,
            DiagnosticAckCode::InvalidSourceAddress,
        );
        return TcpVerdict::Close;
    }
    if dm.target_address.0 != s.own_address {
        send_diag_ack(
            s,
            PayloadType::DiagnosticMessageNegativeAcknowledge,
            tester,
            DiagnosticAckCode::UnknownTargetAddress,
        );
        return TcpVerdict::KeepOpen;
    }

    send_diag_ack(
        s,
        PayloadType::DiagnosticMessagePositiveAcknowledge,
        tester,
        DiagnosticAckCode::RoutingConfirmationAck,
    );

    // `<= 0` means the dispatch produced no response; a negative return maps to
    // zero written bytes, which the guard below then skips.
    let written =
        usize::try_from((s.cb.on_uds_request)(dm.user_data, &mut s.uds_resp_buf[..])).unwrap_or(0);
    if written > 0 {
        let len = written.min(UDS_RESP_CAP);
        let msg = Message::diagnostic_message(
            PROTOCOL_VERSION,
            LogicalAddress(s.own_address),
            LogicalAddress(tester),
            &s.uds_resp_buf[..len],
        );
        send_tcp_msg(&s.cb, s.tx_buf, &msg);
    }
    TcpVerdict::KeepOpen
}

fn handle_tcp_frame(s: &mut Session<'_>, header: &Header, payload_bytes: &[u8]) -> TcpVerdict {
    match Payload::decode(payload_bytes, header.payload_type) {
        Ok(Payload::RoutingActivationRequest(req)) => handle_routing_activation(s, &req),
        Ok(Payload::DiagnosticMessage(dm)) => handle_diagnostic_message(s, &dm),
        // Valid-but-unserved payload type on the data socket: discard per
        // the NACK 0x01 action, connection stays up.
        Ok(_) | Err(_) => {
            send_tcp_nack(&s.cb, s.tx_buf, NackCode::UnknownPayloadType);
            TcpVerdict::KeepOpen
        }
    }
}

fn handle_tcp_bytes(
    s: &mut Session<'_>,
    rx_len: &mut usize,
    rx_buf: &mut [u8; TCP_RX_CAP],
    data: &[u8],
) -> TcpVerdict {
    if *rx_len + data.len() > TCP_RX_CAP {
        send_tcp_nack(&s.cb, s.tx_buf, NackCode::OutOfMemory);
        return TcpVerdict::Close;
    }
    rx_buf[*rx_len..*rx_len + data.len()].copy_from_slice(data);
    *rx_len += data.len();

    loop {
        // A frame longer than the reassembly buffer can never complete;
        // reject it up front instead of stalling until overflow.
        if *rx_len >= Header::SIZE
            && let Ok((header, _rest)) = Header::decode(&rx_buf[..Header::SIZE])
            && header.payload_length as usize > MAX_RX_PAYLOAD
        {
            send_tcp_nack(&s.cb, s.tx_buf, NackCode::MessageTooLarge);
            return TcpVerdict::Close;
        }

        let consumed = match try_frame(&rx_buf[..*rx_len]) {
            Ok(None) => return TcpVerdict::KeepOpen,
            Err(_) => {
                send_tcp_nack(&s.cb, s.tx_buf, NackCode::IncorrectPatternFormat);
                return TcpVerdict::Close;
            }
            Ok(Some((frame, consumed))) => {
                let verdict = handle_tcp_frame(s, &frame.header, frame.payload);
                if verdict != TcpVerdict::KeepOpen {
                    return verdict;
                }
                consumed
            }
        };

        rx_buf.copy_within(consumed..*rx_len, 0);
        *rx_len -= consumed;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::vec::Vec;

    use super::*;
    use crate::messages::{DiagnosticMessageAck, RoutingActivationResponseCode};

    /// Captured platform output. One global (fn-pointer callbacks cannot
    /// close over state); [`fresh_entity`] serializes tests around it.
    #[derive(Default)]
    struct Capture {
        udp: Vec<(u32, u16, Vec<u8>)>,
        tcp: Vec<Vec<u8>>,
        uds_requests: Vec<Vec<u8>>,
    }

    static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
        udp: Vec::new(),
        tcp: Vec::new(),
        uds_requests: Vec::new(),
    });
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    const OWN_ADDRESS: u16 = 0x4010;
    const TESTER: u16 = 0x0E00;
    const VIN: [u8; VIN_LEN] = *b"TESTVIN0000000001";

    fn capture() -> MutexGuard<'static, Capture> {
        CAPTURE.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn callbacks() -> Callbacks {
        Callbacks {
            send_udp: |addr, port, data| {
                capture().udp.push((addr, port, data.to_vec()));
                0
            },
            send_tcp: |data| {
                capture().tcp.push(data.to_vec());
                0
            },
            // Echo dispatcher: respond `55` ++ request.
            on_uds_request: |request, response_out| {
                capture().uds_requests.push(request.to_vec());
                response_out[0] = 0x55;
                response_out[1..=request.len()].copy_from_slice(request);
                (request.len() + 1) as i32
            },
        }
    }

    fn fresh_entity() -> (MutexGuard<'static, ()>, Entity) {
        let guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        *capture() = Capture::default();
        let mut entity = Entity::new();
        entity.init(
            &EntityConfig {
                vin: VIN,
                eid: [1, 2, 3, 4, 5, 6],
                gid: [0; GID_LEN],
                logical_address: OWN_ADDRESS,
            },
            callbacks(),
        );
        (guard, entity)
    }

    fn encode(payload_type: PayloadType, payload: Payload<'_>) -> Vec<u8> {
        let msg = framed(payload_type, payload).unwrap();
        let mut buf = [0u8; TX_CAP];
        let len = encode_into(&mut buf, &msg).unwrap();
        buf[..len].to_vec()
    }

    fn decode_single(bytes: &[u8]) -> (Header, Vec<u8>) {
        let (frame, consumed) = try_frame(bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len(), "reply must be one whole frame");
        (frame.header, frame.payload.to_vec())
    }

    fn activate(entity: &mut Entity) {
        let req = encode(
            PayloadType::RoutingActivationRequest,
            Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address: LogicalAddress(TESTER),
                activation_type: ActivationTypeCode::Default,
                reserved: [0; 4],
                reserved_vehicle_manufacturer: None,
            }),
        );
        assert_eq!(entity.on_tcp_rx(&req), TcpVerdict::KeepOpen);
    }

    #[test]
    fn vehicle_identification_request_gets_announcement() {
        let (_guard, mut entity) = fresh_entity();
        let req = encode(
            PayloadType::VehicleIdentificationRequest,
            Payload::VehicleIdentificationRequest,
        );
        entity.on_udp_rx(0x0A00_0001, 55555, &req);

        let udp = &capture().udp;
        assert_eq!(udp.len(), 1);
        let (addr, port, reply) = &udp[0];
        assert_eq!((*addr, *port), (0x0A00_0001, 55555));
        let (header, payload) = decode_single(reply);
        match Payload::decode(&payload, header.payload_type).unwrap() {
            Payload::VehicleAnnouncement(va) => {
                assert_eq!(va.vin, VIN);
                assert_eq!(va.logical_address, LogicalAddress(OWN_ADDRESS));
                assert_eq!(va.group_id, None);
            }
            other => panic!("expected announcement, got {other:?}"),
        }
    }

    #[test]
    fn routing_activation_then_diagnostic_message_round_trip() {
        let (_guard, mut entity) = fresh_entity();
        activate(&mut entity);

        {
            let tcp = &capture().tcp;
            assert_eq!(tcp.len(), 1);
            let (header, payload) = decode_single(&tcp[0]);
            match Payload::decode(&payload, header.payload_type).unwrap() {
                Payload::RoutingActivationResponse(resp) => {
                    assert_eq!(
                        resp.routing_activation_response_code,
                        RoutingActivationResponseCode::RoutingSuccessfullyActivated
                    );
                    assert_eq!(resp.logical_address_tester, LogicalAddress(TESTER));
                }
                other => panic!("expected activation response, got {other:?}"),
            }
        }

        let diag = encode(
            PayloadType::DiagnosticMessage,
            Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: LogicalAddress(TESTER),
                target_address: LogicalAddress(OWN_ADDRESS),
                user_data: &[0x22, 0xF1, 0x8A],
            }),
        );
        assert_eq!(entity.on_tcp_rx(&diag), TcpVerdict::KeepOpen);

        let cap = capture();
        assert_eq!(cap.uds_requests, [std::vec![0x22, 0xF1, 0x8A]]);
        // Activation response + positive ACK + UDS response.
        assert_eq!(cap.tcp.len(), 3);
        let (ack_header, ack_payload) = decode_single(&cap.tcp[1]);
        assert_eq!(
            ack_header.payload_type,
            PayloadType::DiagnosticMessagePositiveAcknowledge
        );
        assert!(matches!(
            Payload::decode(&ack_payload, ack_header.payload_type).unwrap(),
            Payload::DiagnosticMessageAck(DiagnosticMessageAck { .. })
        ));
        let (resp_header, resp_payload) = decode_single(&cap.tcp[2]);
        match Payload::decode(&resp_payload, resp_header.payload_type).unwrap() {
            Payload::DiagnosticMessage(dm) => {
                assert_eq!(dm.source_address, LogicalAddress(OWN_ADDRESS));
                assert_eq!(dm.target_address, LogicalAddress(TESTER));
                assert_eq!(dm.user_data, &[0x55, 0x22, 0xF1, 0x8A]);
            }
            other => panic!("expected diagnostic response, got {other:?}"),
        }
    }

    #[test]
    fn diagnostic_message_without_activation_closes() {
        let (_guard, mut entity) = fresh_entity();
        let diag = encode(
            PayloadType::DiagnosticMessage,
            Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: LogicalAddress(TESTER),
                target_address: LogicalAddress(OWN_ADDRESS),
                user_data: &[0x3E, 0x00],
            }),
        );
        assert_eq!(entity.on_tcp_rx(&diag), TcpVerdict::Close);

        let cap = capture();
        assert!(cap.uds_requests.is_empty());
        assert_eq!(cap.tcp.len(), 1);
        let (header, _payload) = decode_single(&cap.tcp[0]);
        assert_eq!(
            header.payload_type,
            PayloadType::DiagnosticMessageNegativeAcknowledge
        );
    }

    #[test]
    fn fragmented_tcp_stream_reassembles() {
        let (_guard, mut entity) = fresh_entity();
        let req = encode(
            PayloadType::RoutingActivationRequest,
            Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address: LogicalAddress(TESTER),
                activation_type: ActivationTypeCode::Default,
                reserved: [0; 4],
                reserved_vehicle_manufacturer: None,
            }),
        );
        let split = req.len() / 2;
        assert_eq!(entity.on_tcp_rx(&req[..split]), TcpVerdict::KeepOpen);
        assert!(capture().tcp.is_empty(), "half a frame must not respond");
        assert_eq!(entity.on_tcp_rx(&req[split..]), TcpVerdict::KeepOpen);
        assert_eq!(capture().tcp.len(), 1);
    }

    #[test]
    fn oversize_frame_is_rejected_up_front() {
        let (_guard, mut entity) = fresh_entity();
        activate(&mut entity);

        let mut header = [0u8; Header::SIZE];
        let msg = Header::new(
            PROTOCOL_VERSION,
            PayloadType::DiagnosticMessage,
            (MAX_RX_PAYLOAD + 1) as u32,
        );
        let mut writer: &mut [u8] = &mut header;
        msg.encode(&mut writer).unwrap();

        assert_eq!(entity.on_tcp_rx(&header), TcpVerdict::Close);
        let cap = capture();
        let (nack_header, nack_payload) = decode_single(cap.tcp.last().unwrap());
        assert!(matches!(
            Payload::decode(&nack_payload, nack_header.payload_type).unwrap(),
            Payload::DoIPNack(NackCode::MessageTooLarge)
        ));
    }

    #[test]
    fn second_tester_is_denied() {
        let (_guard, mut entity) = fresh_entity();
        activate(&mut entity);

        let req = encode(
            PayloadType::RoutingActivationRequest,
            Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address: LogicalAddress(TESTER + 1),
                activation_type: ActivationTypeCode::Default,
                reserved: [0; 4],
                reserved_vehicle_manufacturer: None,
            }),
        );
        assert_eq!(entity.on_tcp_rx(&req), TcpVerdict::Close);

        let cap = capture();
        let (header, payload) = decode_single(cap.tcp.last().unwrap());
        match Payload::decode(&payload, header.payload_type).unwrap() {
            Payload::RoutingActivationResponse(resp) => assert_eq!(
                resp.routing_activation_response_code,
                RoutingActivationResponseCode::DeniedSourceAddressAlreadyActivated
            ),
            other => panic!("expected activation response, got {other:?}"),
        }
    }
}
