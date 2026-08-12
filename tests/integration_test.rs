//! End-to-end integration tests that exercise a real client and server over an
//! actual TCP socket on localhost.
//!
//! The server side is driven through [`Server::handle_client_connection`], which is
//! the same per-connection entry point [`Server::run_server`] uses internally. This
//! lets each test bind to `127.0.0.1:0` (an OS-assigned ephemeral port) instead of the
//! fixed [`simple_doip::TCP_PORT`] that `run_server` hardcodes, keeping the tests free
//! of port collisions.
//!
//! The client side uses the real [`Client`] API, but with a small test-only
//! [`Connector`] implementation instead of [`simple_doip::connection::ConnectorSocket`].
//! `ConnectorSocket` refuses to connect anywhere except [`simple_doip::TCP_PORT`], which
//! is incompatible with binding to an ephemeral port; substituting the connector is the
//! documented extension point for exactly this situation (see the "Custom
//! Implementation" example in `src/connection.rs`) and requires no changes to `src/`.

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use simple_doip::{
    Error, LogicalAddress,
    client::{AddressType, Client, ClientOptions, RoutingActivationOptions},
    connection::Connector,
    message_codec::MessageCodec,
    messages::{
        ActivationTypeCode, DiagnosticAckCode, DiagnosticMessage, Encode, OwnedMessage,
        OwnedPayload, ProtocolVersion, RoutingActivationRequest, RoutingActivationResponseCode,
    },
    server::{ResponseWriter, Server, ServerConnectionHandler},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    task::JoinHandle,
};
use tokio_util::codec::{FramedRead, FramedWrite};

/// Generous but finite bound for every await in these tests, so a regression that hangs
/// the client or server fails the test quickly instead of hanging CI.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The ECU logical address the test server identifies as.
const SERVER_LOGICAL_ADDRESS: LogicalAddress = LogicalAddress(0x0001);
/// The tester (client) logical address used by test clients.
const CLIENT_LOGICAL_ADDRESS: LogicalAddress = LogicalAddress(0x0E01);

/// Await `fut`, panicking with a descriptive message if it doesn't complete within
/// [`TEST_TIMEOUT`].
async fn with_timeout<F: std::future::Future>(context: &str, fut: F) -> F::Output {
    tokio::time::timeout(TEST_TIMEOUT, fut)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for: {context}"))
}

/// Send the positive `DiagnosticMessageAck` that ISO 13400 requires before any UDS
/// response, addressed from this entity back to the requesting tester and echoing the
/// request's bytes.
///
/// Every handler below needs this exact ack and differs only in what it does *after*
/// it, so it lives here once instead of being re-derived (and re-mistyped) per handler.
async fn send_positive_ack(
    handler: &impl ServerConnectionHandler,
    message: &DiagnosticMessage<'_>,
    responses: &mut dyn ResponseWriter,
) -> Result<(), Error> {
    responses
        .send(OwnedMessage::diagnostic_message_ack(
            handler.protocol_version(),
            handler.get_logical_address(),
            message.source_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            message.user_data.to_vec(),
        ))
        .await
}

/// Test [`ServerConnectionHandler`]. Always accepts routing activation and positively
/// acknowledges diagnostic messages, recording the last diagnostic payload it received
/// so tests can assert on it (the [`Client`] facade only surfaces ack success/failure,
/// not the acknowledged bytes, so the round-trip content is verified server-side).
struct TestHandler {
    last_diagnostic_payload: Arc<Mutex<Option<Vec<u8>>>>,
    routing_activation_requests: Arc<AtomicUsize>,
}

#[async_trait]
impl ServerConnectionHandler for TestHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error> {
        self.routing_activation_requests
            .fetch_add(1, Ordering::SeqCst);
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
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        *self.last_diagnostic_payload.lock().unwrap() = Some(message.user_data.to_vec());
        send_positive_ack(self, message, responses).await
    }
}

/// Handle to a test server instance running in the background.
struct TestServer {
    addr: SocketAddr,
    last_diagnostic_payload: Arc<Mutex<Option<Vec<u8>>>>,
    routing_activation_requests: Arc<AtomicUsize>,
    accept_loop: JoinHandle<()>,
}

impl TestServer {
    /// Assert the accept loop survived the test, then shut it down.
    ///
    /// Because the accept loop awaits connections inline (like `run_server`), a panic
    /// escaping `handle_client_connection` kills the whole loop task; this final check
    /// turns that into an explicit test failure even if the test's other assertions
    /// happened to pass first.
    async fn shutdown(self) {
        assert!(
            !self.accept_loop.is_finished(),
            "server accept loop died during the test (a connection handler panicked?)"
        );
        self.accept_loop.abort();
        // Await the aborted task so the test doesn't leave it dangling; cancellation
        // reports a JoinError, which is the expected outcome here.
        let _ = self.accept_loop.await;
    }
}

/// Start a [`Server`] listening on an OS-assigned localhost port.
///
/// This mirrors [`Server::run_server`]'s accept loop, but binds to `127.0.0.1:0` so
/// tests never race over a fixed port. Like `run_server` (see `src/server.rs`), each
/// accepted connection is awaited INLINE in the accept loop - no per-connection task -
/// so a panic escaping `handle_client_connection` kills the accept loop here exactly as
/// it would kill `run_server` in production. That parity is what lets tests 3 and 4
/// catch a regression that reintroduces a panic on malformed/unsupported input: the
/// follow-up "fresh client can still connect" step would fail.
async fn start_server() -> TestServer {
    let last_diagnostic_payload = Arc::new(Mutex::new(None));
    let routing_activation_requests = Arc::new(AtomicUsize::new(0));
    let handler = TestHandler {
        last_diagnostic_payload: Arc::clone(&last_diagnostic_payload),
        routing_activation_requests: Arc::clone(&routing_activation_requests),
    };

    let (addr, accept_loop) = start_server_with(handler).await;

    TestServer {
        addr,
        last_diagnostic_payload,
        routing_activation_requests,
        accept_loop,
    }
}

/// Start a [`Server`] with a caller-supplied handler on an OS-assigned localhost port.
/// [`start_server`] delegates here; tests needing a handler other than [`TestHandler`]
/// call this directly.
///
/// The accept loop awaits each connection INLINE, mirroring `run_server`, so a panic
/// escaping a handler kills the loop here exactly as it would in production - which is
/// what [`TestServer::shutdown`] asserts against.
async fn start_server_with<H>(handler: H) -> (SocketAddr, JoinHandle<()>)
where
    H: ServerConnectionHandler + Send + Sync + 'static,
{
    let server = Server::new(handler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            // Disable Nagle on the server side. `ConnectorSocket` already does this for
            // the client (`src/connection.rs`), but nothing does it for an accepted
            // connection, so consecutive small responses stall on the tester's delayed
            // ACK - measured at ~40ms between two 5-byte writes on loopback. That is a
            // transport artifact with nothing to say about handler behavior, and it
            // would otherwise swamp the sub-50ms timings
            // `handler_holds_pending_wait_open_between_sends` asserts on. Setting it here
            // rather than in `src/` keeps this a test-only change; whether `run_server`
            // itself should set it is a separate question about live P2 timing.
            let _ = stream.set_nodelay(true);
            // Await the connection inline, sequentially, matching `run_server`'s
            // control flow. `run_server` logs a handler error and keeps accepting;
            // mirror that by discarding the error here.
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    (addr, accept_loop)
}

/// Read one framed message off a raw socket, as an [`OwnedMessage`].
///
/// Raw rather than through [`Client`], because the `Client` facade consumes the
/// `DiagnosticMessageAck` internally and never surfaces it - and the ack is exactly what
/// the multi-response tests assert on.
///
/// This and the two send helpers below take an already-split [`FramedRead`]/
/// [`FramedWrite`] half rather than a bare [`TcpStream`], because a codec must not be
/// reconstructed per call: a fresh `FramedRead` drops whatever the previous one
/// buffered, which silently loses a message when two arrive in one TCP segment.
async fn read_message<R>(framed: &mut FramedRead<R, MessageCodec>) -> OwnedMessage
where
    R: AsyncRead + Unpin,
{
    with_timeout("read message", framed.next())
        .await
        .expect("stream closed before a message arrived")
        .expect("decode message")
}

/// Send a routing activation request over a raw socket.
async fn send_routing_activation<W>(
    framed: &mut FramedWrite<W, MessageCodec>,
    source_address: LogicalAddress,
) where
    W: AsyncWrite + Unpin,
{
    let request = OwnedMessage::routing_activation_request(
        ProtocolVersion::V2012,
        source_address,
        ActivationTypeCode::Default,
        None,
    );
    with_timeout("send routing activation", framed.send(&request))
        .await
        .expect("send routing activation");
}

/// Send a diagnostic message carrying `user_data` over a raw socket.
async fn send_diagnostic_message<W>(
    framed: &mut FramedWrite<W, MessageCodec>,
    source_address: LogicalAddress,
    user_data: &[u8],
) where
    W: AsyncWrite + Unpin,
{
    let request = OwnedMessage::diagnostic_message(
        ProtocolVersion::V2012,
        source_address,
        SERVER_LOGICAL_ADDRESS,
        user_data.to_vec(),
    );
    with_timeout("send diagnostic message", framed.send(&request))
        .await
        .expect("send diagnostic message");
}

/// Test-only [`Connector`] that dials whatever address it's given, unlike
/// [`simple_doip::connection::ConnectorSocket`] which requires
/// [`simple_doip::TCP_PORT`]. This is the same extension mechanism documented in
/// `src/connection.rs` for users who need a non-standard port.
#[derive(Clone, Copy, Debug)]
struct TestConnector;

#[async_trait]
impl Connector for TestConnector {
    async fn establish_connection(
        gateway_address: SocketAddr,
    ) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> {
        let stream =
            tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(gateway_address)).await??;
        stream.set_nodelay(true)?;
        Ok(stream.into_split())
    }
}

/// Build [`ClientOptions`] that connect to `server_addr` and automatically perform
/// routing activation on [`Client::connect`].
fn client_options(server_addr: SocketAddr) -> ClientOptions {
    ClientOptions {
        server_address: server_addr,
        server_logical_address: SERVER_LOGICAL_ADDRESS,
        server_physical_address: SERVER_LOGICAL_ADDRESS,
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: CLIENT_LOGICAL_ADDRESS,
        protocol_version: ProtocolVersion::V2012,
        routing_activation_options: Some(RoutingActivationOptions {
            activation_type: ActivationTypeCode::Default,
            oem_specific: None,
        }),
    }
}

/// Connect a fresh client to `server_addr`, performing routing activation, and assert
/// that the server actually saw and (successfully) processed a routing activation
/// request for it.
async fn connect_and_activate(
    server_addr: SocketAddr,
    server: &TestServer,
) -> Client<TestConnector> {
    let requests_before = server.routing_activation_requests.load(Ordering::SeqCst);
    let client = with_timeout(
        "client connect + routing activation",
        Client::<TestConnector>::connect(client_options(server_addr)),
    )
    .await
    .expect("client should connect and activate routing successfully");

    // `Client::connect` doesn't surface the routing activation response code to the
    // caller (it only fails the whole `connect()` call on a hard error), so confirm
    // activation genuinely happened end-to-end by checking that the server's
    // `routing_activation` handler - which always returns
    // `RoutingSuccessfullyActivated` - was actually invoked.
    assert_eq!(
        server.routing_activation_requests.load(Ordering::SeqCst),
        requests_before + 1,
        "server should have processed exactly one routing activation request"
    );
    client
}

/// Test 1: a client can connect to the server and successfully perform routing
/// activation.
#[tokio::test]
async fn routing_activation_succeeds() {
    let server = start_server().await;

    let client = connect_and_activate(server.addr, &server).await;

    with_timeout("client shutdown", client.shut_down()).await;
    server.shutdown().await;
}

/// Test 2: an activated client can send a diagnostic message and receive the
/// server's positive acknowledgement, and the bytes the server received match what
/// the client sent.
#[tokio::test]
async fn diagnostic_message_round_trip() {
    let server = start_server().await;
    let mut client = connect_and_activate(server.addr, &server).await;

    // UDS Diagnostic Session Control: extended diagnostic session.
    let request_bytes = vec![0x10, 0x03];

    let send_result = with_timeout(
        "send_diagnostic_message",
        client.send_diagnostic_message(AddressType::Physical, request_bytes.clone()),
    )
    .await;
    assert!(
        send_result.is_ok(),
        "expected a positive ACK for the diagnostic message, got {send_result:?}"
    );

    let received = server
        .last_diagnostic_payload
        .lock()
        .unwrap()
        .clone()
        .expect("server handler should have recorded the diagnostic payload");
    assert_eq!(
        received, request_bytes,
        "server should receive exactly the bytes the client sent"
    );

    with_timeout("client shutdown", client.shut_down()).await;
    server.shutdown().await;
}

/// Read from `stream` until EOF or an error, with a bound on how long to wait. Returns
/// once the connection is confirmed closed from the peer's side.
async fn wait_for_connection_close(stream: &mut TcpStream) {
    let mut buf = [0u8; 64];
    with_timeout("peer closing the raw connection", async {
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return, // Clean EOF or reset/aborted: connection is closed.
                Ok(_) => {}               // Unexpected data; keep draining until close.
            }
        }
    })
    .await;
}

/// Test 3: a well-formed `DoIP` header carrying a payload type the server doesn't
/// support (`DiagnosticPowerModeInfoRequest`, 0x4003) must not take the server down -
/// and, more specifically, must not tear down the connection it arrived on.
///
/// Unlike a framing-fatal error, an unsupported payload type is now RECOVERABLE at the
/// codec layer (see `MessageCodec::decode` / `MessageError::is_framing_fatal`): the bad
/// frame is skipped and consumed, and decoding resumes on the very next frame in the
/// SAME connection. This test proves exactly that by writing the unsupported frame
/// followed immediately by a well-formed `RoutingActivationRequest` on one raw
/// `TcpStream`, then reading the server's routing activation response back on that same
/// stream. Under the pre-fix codec the unsupported frame's recoverable error would
/// propagate out of `decode`, the connection would be torn down, and no response would
/// ever arrive - so this test fails without the fix.
#[tokio::test]
async fn unsupported_payload_type_does_not_kill_server() {
    let server = start_server().await;

    let mut raw_stream = with_timeout("raw connect", TcpStream::connect(server.addr))
        .await
        .expect("raw TCP connection should succeed");

    // Header: version 0x02 (V2012), correct inverse 0xFD, payload type
    // DiagnosticPowerModeInfoRequest (0x4003), payload length 0.
    let unsupported_frame = [0x02, 0xFD, 0x40, 0x03, 0x00, 0x00, 0x00, 0x00];

    // A well-formed routing activation request, built via the crate's own API rather
    // than hand-rolled bytes.
    let routing_activation_request = OwnedMessage::routing_activation_request(
        ProtocolVersion::V2012,
        CLIENT_LOGICAL_ADDRESS,
        ActivationTypeCode::Default,
        None,
    );
    let mut activation_bytes = vec![0u8; routing_activation_request.encoded_size().unwrap()];
    let written = {
        let mut writer: &mut [u8] = &mut activation_bytes;
        routing_activation_request.encode(&mut writer).unwrap()
    };
    activation_bytes.truncate(written);

    // Write both frames back-to-back on the same connection before reading anything
    // back, so the server must decode straight through the unsupported frame to reach
    // the valid one.
    with_timeout(
        "write unsupported-payload frame followed by a valid routing activation request",
        async {
            raw_stream.write_all(&unsupported_frame).await?;
            raw_stream.write_all(&activation_bytes).await
        },
    )
    .await
    .expect("writes should succeed");

    // If the codec incorrectly tore down the connection on the unsupported frame, this
    // read would hang until TEST_TIMEOUT and fail; on the fix, the server skips that
    // frame, decodes the routing activation request right after it, and responds.
    let mut response_buf = [0u8; 64];
    let read = with_timeout(
        "read routing activation response",
        raw_stream.read(&mut response_buf),
    )
    .await
    .expect("read should succeed");
    assert!(
        read > 0,
        "server should have sent a routing activation response"
    );
    assert_eq!(
        server.routing_activation_requests.load(Ordering::SeqCst),
        1,
        "server should have processed the routing activation request that followed the \
         skipped unsupported frame, on the same connection"
    );

    drop(raw_stream);
    server.shutdown().await;
}

/// Test 4: a corrupt header (inverse protocol version doesn't match the protocol
/// version) must not take the server down either. The offending connection is closed,
/// but the server must keep accepting and serving other clients afterwards.
#[tokio::test]
async fn malformed_header_does_not_kill_server() {
    let server = start_server().await;

    let mut raw_stream = with_timeout("raw connect", TcpStream::connect(server.addr))
        .await
        .expect("raw TCP connection should succeed");

    // Header: version 0x02 (V2012), but a corrupt/incorrect inverse (0xFF instead of
    // the expected 0xFD). Payload type/length are irrelevant since header decoding
    // fails before either is interpreted.
    let corrupt_header = [0x02, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    with_timeout(
        "write malformed header",
        raw_stream.write_all(&corrupt_header),
    )
    .await
    .expect("write should succeed");

    wait_for_connection_close(&mut raw_stream).await;

    // The server must still be alive: a brand new client can connect and activate.
    let fresh_client = connect_and_activate(server.addr, &server).await;
    with_timeout("client shutdown", fresh_client.shut_down()).await;
    server.shutdown().await;
}

/// A [`ServerConnectionHandler`] that answers a routing activation request with an
/// alive check response instead of a routing activation response.
struct MisbehavingHandler;

#[async_trait]
impl ServerConnectionHandler for MisbehavingHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error> {
        // Deliberately the wrong payload type for a routing activation request.
        // An alive check response has no special-case arm in
        // `client_inner::process_received_message`, so it falls straight through to the
        // `is_response` guard this test exists to lock.
        let _ = request;
        Ok(OwnedMessage::alive_check_response(
            self.protocol_version(),
            self.get_logical_address(),
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        send_positive_ack(self, message, responses).await
    }
}

/// Test 5: a server that answers routing activation with the wrong payload type must
/// surface `Err(Error::UnexpectedMessageType(_))` rather than panicking or hanging.
///
/// This is a characterization test for the `is_response` guard in
/// `client_inner::process_received_message`: a response whose payload type does not
/// match the pending request must become a typed error. An `AliveCheckResponse` is used
/// as the wrong payload because it has no special-case arm in
/// `process_received_message`, so it reaches that guard directly.
///
/// The handler previously replied with a `DiagnosticMessageAck`, which never reached the
/// guard: the `DiagnosticMessageAck` arm dropped the pending request's oneshot `Sender`
/// and `connect()` returned `Ok`. This test's assertion used to encode that bug as
/// correct behavior; the bug is fixed in the preceding commit and the dropped-`Sender`
/// path now has its own dedicated regression test
/// (`negative_ack_during_routing_activation_does_not_drop_pending_request`).
#[tokio::test]
async fn wrong_routing_activation_response_type_errors_without_panicking() {
    let server = Server::new(MisbehavingHandler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    let result = with_timeout(
        "client connect against a misbehaving server",
        Client::<TestConnector>::connect(client_options(addr)),
    )
    .await;

    assert!(
        matches!(result, Err(Error::UnexpectedMessageType(_))),
        "a wrongly typed routing activation response must surface as UnexpectedMessageType \
         without panicking or hanging; got: {result:?}"
    );

    accept_loop.abort();
    let _ = accept_loop.await;
}

/// A [`ServerConnectionHandler`] that *denies* routing activation, mimicking a
/// `DoIP` entity whose single `TCP_DATA` slot is already held by another tester
/// (for example EnVision polling the same sensor).
struct DenyingRoutingHandler;

#[async_trait]
impl ServerConnectionHandler for DenyingRoutingHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
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
            RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered,
            [0; 4],
            None,
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        send_positive_ack(self, message, responses).await
    }
}

/// A denied routing activation must fail `connect()` with
/// `Error::RoutingActivationDenied(code)` carrying the entity's response code —
/// not return `Ok` (which previously left the denial invisible and drove an
/// eternal reconnect loop once the entity closed the socket).
#[tokio::test]
async fn routing_activation_denial_surfaces_as_error() {
    let server = Server::new(DenyingRoutingHandler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    let result = with_timeout(
        "client connect against a denying server",
        Client::<TestConnector>::connect(client_options(addr)),
    )
    .await;

    assert!(
        matches!(
            result,
            Err(Error::RoutingActivationDenied(
                RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered
            ))
        ),
        "a routing activation denial must surface as RoutingActivationDenied with the \
         reported code; got: {result:?}"
    );

    accept_loop.abort();
    let _ = accept_loop.await;
}

/// A [`ServerConnectionHandler`] that answers a routing activation request with a
/// *negative* diagnostic message ack.
///
/// A negative ack is deliberate: `client_inner`'s `DiagnosticMessageAck` arm returns
/// early for a *positive* ack ("waiting for full response"), so only a negative ack
/// falls through to the `is_response` guard where the mismatch becomes a real error.
struct NackingRoutingHandler;

#[async_trait]
impl ServerConnectionHandler for NackingRoutingHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error> {
        assert!(
            DiagnosticAckCode::UnknownTargetAddress.is_negative_ack(),
            "this test is only meaningful with a genuinely negative ack code"
        );
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            self.get_logical_address(),
            request.source_address,
            DiagnosticAckCode::UnknownTargetAddress,
            Vec::new(),
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        send_positive_ack(self, message, responses).await
    }
}

/// Regression test: an in-flight `AwaitResponse` (here, a routing activation) must not
/// be destroyed by the arrival of a `DiagnosticMessageAck`.
///
/// `client_inner::process_received_message` used to call `self.active_request.take()`
/// unconditionally inside `if let Some(ControlMessage::AwaitAck(..)) = ...`. When the
/// pending request was an `AwaitResponse` the pattern did not match, and the taken
/// value — including its oneshot `Sender` — was dropped at the end of the statement,
/// closing the routing-activation channel. `Client::bind_socket` treats a closed
/// channel as "server does not support routing activation", so `connect()` returned
/// `Ok` and the protocol violation vanished silently.
///
/// With the pending request restored instead of dropped, the negative ack falls through
/// to the `is_response` guard and the client surfaces
/// `Err(Error::UnexpectedMessageType(_))`.
#[tokio::test]
async fn negative_ack_during_routing_activation_does_not_drop_pending_request() {
    let server = Server::new(NackingRoutingHandler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    let result = with_timeout(
        "client connect against a nacking server",
        Client::<TestConnector>::connect(client_options(addr)),
    )
    .await;

    assert!(
        matches!(result, Err(Error::UnexpectedMessageType(_))),
        "a negative DiagnosticMessageAck answering a routing activation must surface as \
         UnexpectedMessageType, not be silently swallowed; got: {result:?}"
    );

    accept_loop.abort();
    let _ = accept_loop.await;
}

/// Regression test: cancelling a `receive_diagnostic_response` future must not brick the
/// client.
///
/// `receive_diagnostic_response` is normally used under a caller-supplied bound -
/// `tokio::time::timeout(..)` around it, or racing it in a `tokio::select!`. Cancelling
/// it drops the caller's half of the oneshot, but the inner task still holds the
/// matching `ControlMessage::AwaitResponse` in `active_request`. The run loop's control
/// branch used to `assert!(self.active_request.is_none())` at that point, so the *next*
/// user request panicked the detached inner task. Nothing surfaced the panic: the
/// control channel simply closed, and every later call returned
/// `Error::ConnectionClosed` forever - an error that reads as "the peer went away".
///
/// The control branch now supersedes any still-pending request (completing it with
/// `Error::RequestSuperseded` rather than dropping its `Sender`) and accepts the new
/// one, so the client keeps working. The assertion below is on the observable
/// consequence from the client's side, since a panic on a detached task cannot
/// propagate into the test.
#[tokio::test]
async fn cancelled_receive_diagnostic_response_does_not_brick_client() {
    let server = start_server().await;
    let mut client = connect_and_activate(server.addr, &server).await;

    // Ask for a diagnostic response with a long inner deadline, then abandon the future
    // well before that deadline. This is the ordinary cancellation shape, and it leaves
    // an `AwaitResponse` pending inside the inner task.
    let cancelled = tokio::time::timeout(
        Duration::from_millis(50),
        client.receive_diagnostic_response(Duration::from_secs(30)),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "the receive future was supposed to be cancelled by the outer timeout, but it \
         completed: {cancelled:?}"
    );

    // The next request lands in the control branch with a request still pending. It must
    // be served normally.
    let send_result = with_timeout(
        "send_diagnostic_message after a cancelled receive",
        client.send_diagnostic_message(AddressType::Physical, vec![0x10, 0x03]),
    )
    .await;
    assert!(
        send_result.is_ok(),
        "a request issued after a cancelled receive_diagnostic_response must still be \
         served; got: {send_result:?} (Err(ConnectionClosed) means the inner task died)"
    );

    with_timeout("client shutdown", client.shut_down()).await;
    server.shutdown().await;
}

/// A [`ServerConnectionHandler`] that answers routing activation normally, but never
/// responds to a diagnostic message at all (the handler future simply never resolves,
/// as if the server received the message and silently went away).
struct SilentOnDiagnosticHandler;

#[async_trait]
impl ServerConnectionHandler for SilentOnDiagnosticHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
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
        // Never resolves: the server received the message but never acknowledges it,
        // so the client must hit its own internal deadline rather than any
        // server-driven signal.
        std::future::pending().await
    }
}

/// Regression test: a timed-out `SendDiagnosticMessage` (`ControlMessage::AwaitAck`)
/// must surface `Error::ResponseTimeoutExceeded`, not `Error::ConnectionClosed`.
///
/// The run loop's deadline branch used to do
/// `active_request.take()` and match only `ControlMessage::AwaitResponse`; when the
/// pending request was actually an `AwaitAck` (as it is for
/// `send_diagnostic_message`), the pattern did not match, so the taken value -
/// including its oneshot `Sender` - was dropped without being told about the timeout.
/// `Client::send_diagnostic_message` then observed the closed channel and mapped it to
/// `Error::ConnectionClosed`, hiding the real cause (a timeout) from the caller.
///
/// With the deadline branch handling `AwaitAck` explicitly and sending
/// `Err(Error::ResponseTimeoutExceeded)`, the caller now sees the correct error.
#[tokio::test]
async fn timed_out_diagnostic_message_ack_surfaces_timeout_not_connection_closed() {
    let server = Server::new(SilentOnDiagnosticHandler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    let mut client = with_timeout(
        "client connect + routing activation",
        Client::<TestConnector>::connect(client_options(addr)),
    )
    .await
    .expect(
        "client should connect and activate routing successfully against a server \
              that behaves normally until the diagnostic message",
    );

    // simple_doip::TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL is 50ms; TEST_TIMEOUT (5s) gives
    // this test comfortable headroom so it observes the client's own deadline rather
    // than racing it.
    let result = with_timeout(
        "send_diagnostic_message against a server that never acks",
        client.send_diagnostic_message(AddressType::Physical, vec![0x10, 0x03]),
    )
    .await;

    assert!(
        matches!(result, Err(Error::ResponseTimeoutExceeded)),
        "a diagnostic message that the server never acks must surface \
         ResponseTimeoutExceeded, not ConnectionClosed or any other error; got: {result:?}"
    );

    accept_loop.abort();
    let _ = accept_loop.await;
}

/// A handler that answers every diagnostic message with a positive
/// `DiagnosticMessageAck` followed by a separate diagnostic-message response.
/// This is the shape `uds_on_ip` requires - it waits for the ack before it reads a
/// response, so a single-message server deadlocks it.
struct AckThenRespondHandler;

#[async_trait]
impl ServerConnectionHandler for AckThenRespondHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
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
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        send_positive_ack(self, message, responses).await?;
        responses
            .send(OwnedMessage::diagnostic_message(
                self.protocol_version(),
                self.get_logical_address(),
                message.source_address,
                vec![0x62, 0xFD, 0x69, 0xAA],
            ))
            .await?;
        Ok(())
    }
}

/// A single diagnostic request must be answerable with two messages on the wire: the
/// `DiagnosticMessageAck` ISO 13400 requires, then the UDS response itself.
#[tokio::test]
async fn handler_can_emit_ack_then_response() {
    let (server_addr, accept_loop) = start_server_with(AckThenRespondHandler).await;
    let mut stream = with_timeout("connect", TcpStream::connect(server_addr))
        .await
        .expect("connect to test server");
    let (rx, tx) = stream.split();
    let mut reader = FramedRead::new(rx, MessageCodec::new());
    let mut writer = FramedWrite::new(tx, MessageCodec::new());

    // Routing activation first, so the server accepts diagnostic messages.
    send_routing_activation(&mut writer, CLIENT_LOGICAL_ADDRESS).await;
    let _activation = read_message(&mut reader).await;

    send_diagnostic_message(&mut writer, CLIENT_LOGICAL_ADDRESS, &[0x22, 0xFD, 0x69]).await;

    let first = read_message(&mut reader).await;
    assert!(
        matches!(first.payload, OwnedPayload::DiagnosticMessageAck(_)),
        "expected DiagnosticMessageAck first, got {:?}",
        first.payload
    );

    let second = read_message(&mut reader).await;
    match second.payload {
        OwnedPayload::DiagnosticMessage(ref diag) => {
            assert_eq!(diag.user_data, vec![0x62, 0xFD, 0x69, 0xAA]);
        }
        other => panic!("expected DiagnosticMessage second, got {other:?}"),
    }

    accept_loop.abort();
    let _ = accept_loop.await;
}

/// A handler that emits two NRC `0x78` "response pending" messages with a real delay
/// between them, then the final positive response. The delay is what distinguishes a
/// held pending wait from a burst: a client that mishandles P2* timing sees the gap,
/// whereas back-to-back writes hide it.
struct HeldPendingHandler;

#[async_trait]
impl ServerConnectionHandler for HeldPendingHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
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
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        send_positive_ack(self, message, responses).await?;

        for _ in 0..2 {
            responses
                .send(OwnedMessage::diagnostic_message(
                    self.protocol_version(),
                    self.get_logical_address(),
                    message.source_address,
                    // 0x7F <requested SID> 0x78 = requestCorrectlyReceived-ResponsePending
                    vec![0x7F, 0x22, 0x78],
                ))
                .await?;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        responses
            .send(OwnedMessage::diagnostic_message(
                self.protocol_version(),
                self.get_logical_address(),
                message.source_address,
                vec![0x62, 0xFD, 0x69, 0xAA],
            ))
            .await?;
        Ok(())
    }
}

/// Margin for the interleaving bounds below. The handler sleeps 50ms between sends;
/// 40ms leaves 10ms of slack for scheduling and socket jitter on a loaded CI box while
/// staying far away from the ~100ms a batched implementation would produce.
///
/// If [`INTERLEAVING_MARGIN`] ever proves too tight under load, raise it - never delete
/// the assertions that use it, since they are the only thing distinguishing a streamed
/// response sequence from a batched one.
const INTERLEAVING_MARGIN: Duration = Duration::from_millis(40);

/// A handler must be able to hold a pending wait open: emit an NRC `0x78`, await real
/// work, then emit more, with the tester observing each message as it is produced rather
/// than all of them at the end.
///
/// This is the requirement that ruled out returning a `Vec<OwnedMessage>` from
/// `diagnostic_message`, so the test is an executable guard on the [`ResponseWriter`]
/// sink design, not a red-green cycle - it is expected to pass as written, and to fail
/// loudly if the sink is ever replaced by a batched return.
///
/// The property that discriminates the two designs is INTERLEAVING, not total duration.
/// A batched rewrite would keep this fixture's sleeps (they are handler logic, not sink
/// logic), push four messages into a `Vec` over the same ~100ms, and only then let the
/// server write them - so the *last* message still arrives at ~100ms either way. What
/// changes is when the *earlier* messages arrive: streamed, the ack is on the wire before
/// the handler's first sleep and the two pendings are 50ms apart; batched, all four land
/// together once the handler returns. Hence the two bounds below.
#[tokio::test]
async fn handler_holds_pending_wait_open_between_sends() {
    let (server_addr, accept_loop) = start_server_with(HeldPendingHandler).await;
    let mut stream = with_timeout("connect", TcpStream::connect(server_addr))
        .await
        .expect("connect to test server");
    let (rx, tx) = stream.split();
    let mut reader = FramedRead::new(rx, MessageCodec::new());
    let mut writer = FramedWrite::new(tx, MessageCodec::new());

    send_routing_activation(&mut writer, CLIENT_LOGICAL_ADDRESS).await;
    let activation = read_message(&mut reader).await;
    assert!(
        matches!(
            activation.payload,
            OwnedPayload::RoutingActivationResponse(_)
        ),
        "routing activation must succeed before any diagnostic message is sent, otherwise \
         the failures below describe the wrong cause; got {:?}",
        activation.payload
    );

    let started = std::time::Instant::now();
    send_diagnostic_message(&mut writer, CLIENT_LOGICAL_ADDRESS, &[0x22, 0xFD, 0x69]).await;

    let ack = read_message(&mut reader).await;
    let ack_at = started.elapsed();
    assert!(matches!(ack.payload, OwnedPayload::DiagnosticMessageAck(_)));

    let mut pending_at = Vec::new();
    for index in 0..2 {
        let pending = read_message(&mut reader).await;
        pending_at.push(started.elapsed());
        match pending.payload {
            OwnedPayload::DiagnosticMessage(ref diag) => {
                assert_eq!(
                    diag.user_data,
                    vec![0x7F, 0x22, 0x78],
                    "message {index} should be an NRC 0x78 pending"
                );
            }
            other => panic!("expected pending DiagnosticMessage, got {other:?}"),
        }
    }

    let final_response = read_message(&mut reader).await;
    match final_response.payload {
        OwnedPayload::DiagnosticMessage(ref diag) => {
            assert_eq!(diag.user_data, vec![0x62, 0xFD, 0x69, 0xAA]);
        }
        other => panic!("expected final DiagnosticMessage, got {other:?}"),
    }

    // Bound 1, and the one that actually catches the regression: the ack is written
    // before the handler's first sleep, so it must arrive almost immediately. A batched
    // return puts nothing on the socket until the handler returns ~100ms later, and this
    // assertion goes red.
    assert!(
        ack_at < INTERLEAVING_MARGIN,
        "the ack arrived {ack_at:?} after the request; a streamed sink delivers it before \
         the handler's first sleep, so anything near the handler's total runtime means \
         responses are being batched and flushed at the end"
    );

    // Bound 2: the two pendings are separated by the handler's 50ms sleep. Batched, they
    // arrive in the same flush and the gap collapses to microseconds.
    let pending_gap = pending_at[1] - pending_at[0];
    assert!(
        pending_gap >= INTERLEAVING_MARGIN,
        "the two pending responses arrived {pending_gap:?} apart (at {:?} and {:?}); the \
         handler sleeps 50ms between them, so a smaller gap means they were flushed \
         together rather than as the handler produced them",
        pending_at[0],
        pending_at[1]
    );

    // Secondary check: the handler's two 50ms waits really happened. This does NOT
    // discriminate streaming from batching - a batched implementation takes just as long
    // overall, because the sleeps are in the handler either way. It only guards against a
    // fixture that quietly stops sleeping, which would make the two bounds above vacuous.
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "responses arrived in {:?}; expected >=100ms of held pending waits",
        started.elapsed()
    );

    accept_loop.abort();
    let _ = accept_loop.await;
}
