//! The UDP half of the `DoIP` entity: answering vehicle-identification probes.
//!
//! Binds an ephemeral UDP port rather than the real
//! [`simple_doip::UDP_DISCOVERY_PORT`] so these tests run in parallel with
//! everything else, and so several of them can run at once without colliding.

use async_trait::async_trait;
use simple_doip::{
    Error, LogicalAddress,
    messages::{
        Decode, DiagnosticMessage, Message, OwnedMessage, Payload, RoutingActivationRequest,
        RoutingActivationResponseCode,
    },
    server::{ResponseWriter, Server, ServerConnectionHandler},
};
use std::time::Duration;
use tokio::net::UdpSocket;

/// Generous but finite bound for every await in these tests, so a regression that
/// stops the responder answering fails the test quickly instead of hanging CI.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

const SERVER_LOGICAL_ADDRESS: LogicalAddress = LogicalAddress(0x0001);
const TEST_VIN: [u8; 17] = *b"MVIS0000000000001";
const TEST_ENTITY_ID: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

/// Minimal handler: it supplies the identity fields the default
/// `received_vehicle_identification_request` reads, and stubs out the TCP-side
/// methods these tests never reach.
struct IdentityHandler;

#[async_trait]
impl ServerConnectionHandler for IdentityHandler {
    fn get_vin(&self) -> [u8; 17] {
        TEST_VIN
    }
    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }
    fn get_entity_id(&self) -> [u8; 6] {
        TEST_ENTITY_ID
    }
    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error> {
        Ok(OwnedMessage::routing_activation_response(
            self.protocol_version(),
            request.source_address,
            self.get_logical_address(),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0; 4],
            None,
        ))
    }

    async fn diagnostic_message(
        &self,
        _message: &DiagnosticMessage<'_>,
        _responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// A `VehicleIdentificationRequest` on the wire: 8-byte header, no payload.
/// Protocol version 0x02 with its inverse 0xFD, payload type 0x0001, length 0.
const VEHICLE_IDENTIFICATION_REQUEST: [u8; 8] = [0x02, 0xFD, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];

/// Too short to be a `DoIP` generic header, so `Message::decode` rejects it. Any
/// host on the network can send this, so the responder must log and carry on.
const TRUNCATED_DATAGRAM: [u8; 3] = [0x02, 0xFD, 0x00];

/// Bind an ephemeral responder socket, start [`Server::run_udp_responder`] on it,
/// and return the address a probing client should send to.
async fn start_udp_responder() -> std::net::SocketAddr {
    let server_socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind server");
    let server_addr = server_socket.local_addr().expect("server addr");

    let server = Server::new(IdentityHandler).expect("construct server");
    tokio::spawn(async move {
        let _ = server.run_udp_responder(server_socket).await;
    });

    server_addr
}

/// Await one datagram on `client` and assert it is this entity's identification
/// response.
async fn expect_identification_response(client: &UdpSocket) {
    let mut buf = [0u8; 256];
    let (len, _from) = tokio::time::timeout(TEST_TIMEOUT, client.recv_from(&mut buf))
        .await
        .expect("timed out waiting for identification response")
        .expect("recv identification response");

    // `Decode` is implemented for the BORROWED `Message<'a>`, not `OwnedMessage`,
    // and returns (message, remaining_bytes) — not a bare message.
    let (message, _rest) = Message::decode(&buf[..len]).expect("decode identification response");
    // ISO 13400-2 has a single wire payload type (0x0004) for both the
    // unsolicited announcement and the directed reply, so `Payload::decode`
    // always yields `VehicleAnnouncement` — never the `VehicleIdentificationResponse`
    // variant the responder constructs. Both encode identically.
    match message.payload {
        Payload::VehicleAnnouncement(response) => {
            assert_eq!(response.vin, TEST_VIN);
            assert_eq!(response.entity_id, TEST_ENTITY_ID);
            assert_eq!(response.logical_address, SERVER_LOGICAL_ADDRESS);
        }
        other => panic!("expected a vehicle identification response, got {other:?}"),
    }
}

#[tokio::test]
async fn udp_responder_answers_a_vehicle_identification_request() {
    let server_addr = start_udp_responder().await;

    let client = UdpSocket::bind("127.0.0.1:0").await.expect("bind client");
    client
        .send_to(&VEHICLE_IDENTIFICATION_REQUEST, server_addr)
        .await
        .expect("send identification request");

    expect_identification_response(&client).await;
}

/// A malformed datagram must not take the responder down. Anyone on the network
/// can reach a UDP responder, so a single bad probe killing the loop would let a
/// stray packet make the entity permanently undiscoverable.
#[tokio::test]
async fn udp_responder_survives_a_malformed_datagram() {
    let server_addr = start_udp_responder().await;

    let client = UdpSocket::bind("127.0.0.1:0").await.expect("bind client");
    client
        .send_to(&TRUNCATED_DATAGRAM, server_addr)
        .await
        .expect("send truncated datagram");
    client
        .send_to(&VEHICLE_IDENTIFICATION_REQUEST, server_addr)
        .await
        .expect("send identification request");

    // The good probe still gets answered, so the bad one was skipped rather than
    // fatal. It also proves the responder sent nothing for the bad datagram: this
    // read would otherwise pick up that spurious reply and fail to decode it.
    expect_identification_response(&client).await;
}
