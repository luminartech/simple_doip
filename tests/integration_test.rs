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
use simple_doip::{
    Error, LogicalAddress,
    client::{AddressType, Client, ClientOptions, RoutingActivationOptions},
    connection::Connector,
    messages::{
        ActivationTypeCode, DiagnosticAckCode, DiagnosticMessage, Encode, OwnedMessage,
        ProtocolVersion, RoutingActivationRequest, RoutingActivationResponseCode,
    },
    server::{Server, ServerConnectionHandler},
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
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    task::JoinHandle,
};

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
    ) -> Result<OwnedMessage, Error> {
        *self.last_diagnostic_payload.lock().unwrap() = Some(message.user_data.to_vec());
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            message.source_address,
            message.target_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            message.user_data.to_vec(),
        ))
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
            // Await the connection inline, sequentially, matching `run_server`'s
            // control flow. `run_server` logs a handler error and keeps accepting;
            // mirror that by discarding the error here.
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    TestServer {
        addr,
        last_diagnostic_payload,
        routing_activation_requests,
        accept_loop,
    }
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
                Ok(0) => return,   // Clean EOF: server closed the connection.
                Ok(_) => continue, // Unexpected data; keep draining until close.
                Err(_) => return,  // Reset/aborted: also an acceptable "closed" outcome.
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
    ) -> Result<OwnedMessage, Error> {
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            message.source_address,
            message.target_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            message.user_data.to_vec(),
        ))
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
    ) -> Result<OwnedMessage, Error> {
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            message.source_address,
            message.target_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            message.user_data.to_vec(),
        ))
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
    ) -> Result<OwnedMessage, Error> {
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
