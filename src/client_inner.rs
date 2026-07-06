//! User → Client → `control_sender` → Inner → SocketManager.sender → TCP Socket → Server
//! User ← Client ← `update_receiver` ← Inner ← SocketManager.receiver ← TCP Socket ← Server
use crate::{
    Error,
    client::ClientOptions,
    connection_state::ConnectionState,
    messages::{MessageError, OwnedMessage, Payload},
    socket_manager::SocketManager,
};
use std::{format, future, net::SocketAddr};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tracing::{debug, trace};

/// Messages used to control the `DoIP` entities
#[allow(unused)]
#[derive(Debug)]
pub(super) enum ControlMessage {
    /// No payload
    AliveCheckRequest(oneshot::Sender<Result<(), Error>>),
    AliveCheckResponse(OwnedMessage),
    /// No oneshot needed, the response is sent
    /// and does not need to be awaited
    SendNoResponse(OwnedMessage),

    BindSocket(SocketAddr, oneshot::Sender<Result<u16, Error>>),
    UnbindSocket(oneshot::Sender<Result<(), Error>>),
    RoutingActivation(OwnedMessage, oneshot::Sender<Result<OwnedMessage, Error>>),
    AwaitResponse(
        /// the Request message that was sent
        OwnedMessage,
        /// the response channel to send the response to
        oneshot::Sender<Result<OwnedMessage, Error>>,
    ),

    /// Send diagnostic message and wait for `DoIP` ACK only (not full response)
    SendDiagnosticMessage(OwnedMessage, oneshot::Sender<Result<(), Error>>),

    /// Wait for next diagnostic response (no send)
    ReceiveDiagnosticResponse(
        std::time::Duration,
        oneshot::Sender<Result<OwnedMessage, Error>>,
    ),

    /// Internal: waiting for ACK only (after `SendDiagnosticMessage`)
    AwaitAck(oneshot::Sender<Result<(), Error>>),
}

impl ControlMessage {
    /// Helper method to create control messages with oneshot channels
    fn create_oneshot<T>(
        factory: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Self,
    ) -> (oneshot::Receiver<Result<T, Error>>, Self) {
        let (sender, receiver) = oneshot::channel();
        (receiver, factory(sender))
    }

    /// Create a control message to send a routing activation request
    #[allow(unused)]
    pub fn create_routing_activation_message(
        message: &OwnedMessage,
    ) -> (oneshot::Receiver<Result<OwnedMessage, Error>>, Self) {
        trace!(message = "RoutingActivationRequest");
        Self::create_oneshot(|sender| Self::RoutingActivation(message.clone(), sender))
    }

    /// Builder for the bind socket message
    pub fn create_bind_socket_message(
        gateway_address: SocketAddr,
    ) -> (oneshot::Receiver<Result<u16, Error>>, Self) {
        Self::create_oneshot(|sender| Self::BindSocket(gateway_address, sender))
    }

    /// Create a control message to unbind the TCP socket
    pub fn create_unbind_socket_message() -> (oneshot::Receiver<Result<(), Error>>, Self) {
        Self::create_oneshot(Self::UnbindSocket)
    }

    /// Create a control message to send a diagnostic message and wait for ACK only
    pub fn create_send_diagnostic_message(
        message: OwnedMessage,
    ) -> (oneshot::Receiver<Result<(), Error>>, Self) {
        Self::create_oneshot(|sender| Self::SendDiagnosticMessage(message, sender))
    }

    /// Create a control message to receive the next diagnostic response
    pub fn create_receive_diagnostic_response(
        timeout: std::time::Duration,
    ) -> (oneshot::Receiver<Result<OwnedMessage, Error>>, Self) {
        Self::create_oneshot(|sender| Self::ReceiveDiagnosticResponse(timeout, sender))
    }
}

/// Inner client responsible for the handling of the connection details,
/// including creating 2 channels for sending and receiving messages.
/// It manages its inner state asynchronously, only propagating the
/// results to the outer client when the message is ready.
///
/// The inner state enters an asynchronous loop which runs in the background
pub(super) struct Inner<Conn> {
    client_options: ClientOptions,
    /// MPSC Receiver used to receive control messages from outer client
    control_receiver: mpsc::Receiver<ControlMessage>,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<Result<OwnedMessage, MessageError>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request: Option<ControlMessage>,

    /// Deadline for awaiting a response
    await_response_deadline: Option<tokio::time::Instant>,

    /// Socket manager for TCP data socket if bound
    tcp_data_socket: Option<SocketManager<Conn>>,

    /// Represents the `DoIP` connection state of the socket
    connection_state: ConnectionState,

    /// Whether to keep the inner client running
    ///
    /// This is used to gracefully shut down the inner client
    /// when the outer client is dropped
    run: bool,

    /// Buffer for diagnostic responses that arrive between send and receive calls.
    /// This handles the race condition where the server responds before
    /// `receive_diagnostic_response()` is called.
    pending_diagnostic_response: Option<OwnedMessage>,
}
/// Sender for the control messages sent via the control channel
type ControlSender = mpsc::Sender<ControlMessage>;
/// Receiver for the update messages from the socket
type UpdateReceiver<E> = mpsc::Receiver<Result<OwnedMessage, E>>;

impl<Conn> Inner<Conn>
where
    Conn: crate::connection::Connector + 'static + Send + Sync,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub fn spawn(client_options: ClientOptions) -> (ControlSender, UpdateReceiver<MessageError>) {
        trace!("Spawning inner client");
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        let inner = Inner::<Conn> {
            client_options,
            control_receiver,
            update_sender,
            active_request: None,
            await_response_deadline: None,
            tcp_data_socket: None,
            connection_state: ConnectionState::Listen,
            run: true,
            pending_diagnostic_response: None,
        };
        inner.run();
        (control_sender, update_receiver)
    }

    /// Binds the unicast socket to the specified address.
    /// If a socket already exists, it will be replaced with a new connection
    /// (this handles reconnection after connection loss).
    async fn bind_socket(&mut self, gateway_address: SocketAddr) -> Result<u16, Error> {
        // Close existing socket if any (handles reconnection)
        if let Some(old_socket) = self.tcp_data_socket.take() {
            debug!("Closing existing socket for reconnection");
            old_socket.shut_down().await;
        }

        // Bind a new socket
        let socket_manager: SocketManager<Conn> =
            SocketManager::bind(self.client_options, gateway_address).await?;
        self.connection_state = ConnectionState::Initialized;
        let port = socket_manager.port();
        debug!("Bound socket to port: {}", port);
        self.tcp_data_socket = Some(socket_manager);
        self.run = true;
        Ok(port)
    }

    /// Unbind the socket
    /// This is used to gracefully shut down the socket manager
    async fn unbind_socket(&mut self) -> Result<(), Error> {
        // Check if the socket is already bound
        if let Some(socket_manager) = self.tcp_data_socket.take() {
            self.run = false;
            // Unbind the socket
            socket_manager.shut_down().await;
            Ok(())
        } else {
            Err(Error::SocketNotBound)
        }
    }

    #[allow(unused)]
    /// Send an alive check response, can be sent regardless of if there was a request
    async fn send_alive_check_response(&mut self) -> Result<(), Error> {
        if self.tcp_data_socket.is_none() {
            return Err(Error::SocketNotBound);
        }
        // Send to the tcp_data_socket
        self.send_to_socket(OwnedMessage::alive_check_response(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
        ))
        .await
    }

    /// Send a message to the socket manager
    ///
    /// Errors:
    /// [`Error::SocketNotBound`] - if the socket is not bound
    async fn send_to_socket(&mut self, message: OwnedMessage) -> Result<(), Error> {
        if let Some(socket) = &mut self.tcp_data_socket {
            socket.send(message).await
        } else {
            Err(Error::SocketNotBound)
        }
    }

    async fn receive_socket(
        socket_manager: &mut Option<SocketManager<Conn>>,
    ) -> Result<OwnedMessage, Error> {
        if let Some(receiver) = socket_manager {
            match receiver.receive().await {
                Some(message) => message.map_err(|_| Error::SocketClosedUnexpectedly),
                None => Err(Error::SocketClosedUnexpectedly),
            }
        } else {
            // If we don't have a receiver, we should return a future that never resolves
            future::pending().await
        }
    }

    /// Handle the [`Inner::active_request`] that was set in the [`Inner::run`] loop.
    ///
    /// The `response` is a oneshot channel that is used to (generally) send the response back to the facade client
    ///
    /// ### Diagnostic Acks:
    /// * will not be sent to the user
    /// * will be handled internally
    #[allow(clippy::too_many_lines)]
    async fn handle_control_message(&mut self) {
        // No active request? Nothing to do
        let Some(control_message) = self.active_request.take() else {
            return;
        };

        match control_message {
            ControlMessage::AliveCheckRequest(response) => {
                let send_result = self
                    .send_to_socket(OwnedMessage::alive_check_request(
                        self.client_options.protocol_version,
                    ))
                    .await;
                if response.send(send_result).is_err() {
                    debug!("Failed to send alive check response");
                }
            }
            ControlMessage::AliveCheckResponse(message) => {
                let _ = self
                    .send_to_socket(OwnedMessage::alive_check_response(
                        self.client_options.protocol_version,
                        self.client_options.client_logical_address,
                    ))
                    .await;
                // Don't really have to handle the alive check response
                // it's intended to keep the connection alive and that is handled in the socket manager
                debug!("Received alive check response: {:?}", message);
            }
            ControlMessage::SendNoResponse(message) => {
                let err_msg = format!("Failed to send message: {message:?}");
                // Send the message to the socket
                let send_result = self.send_to_socket(message).await;

                if send_result.is_err() {
                    debug!(err_msg);
                }
            }
            ControlMessage::BindSocket(gateway_address, response) => {
                if response
                    .send(self.bind_socket(gateway_address).await)
                    .is_err()
                {
                    debug!("Failed to send bind socket response");
                }
            }
            ControlMessage::UnbindSocket(response) => {
                if response.send(self.unbind_socket().await).is_err() {
                    debug!("Failed to send unbind socket response");
                }
            }
            ControlMessage::RoutingActivation(message, response) => {
                let send_result = self.send_to_socket(message.clone()).await;

                // Await for the response through the run loop
                if send_result.is_ok() {
                    self.await_response_deadline =
                        Some(tokio::time::Instant::now() + crate::TCP_TIMEOUT_INITIAL_INACTIVITY);
                    self.active_request =
                        Some(ControlMessage::AwaitResponse(message.clone(), response));
                } else if let Err(e) = send_result
                    && response.send(Err(e)).is_err()
                {
                    debug!("Failed to send routing activation error response");
                }
            }
            ControlMessage::AwaitResponse(message, response) => {
                trace!("Awaiting response for message: {:?}", message);
                // This is handled in the run loop while receiving messages
                self.active_request = Some(ControlMessage::AwaitResponse(message, response));
            }
            ControlMessage::SendDiagnosticMessage(message, response) => {
                if self.tcp_data_socket.is_none() {
                    if response.send(Err(Error::SocketNotBound)).is_err() {
                        debug!("Failed to send response: Socket not bound");
                    }
                    return;
                }
                let send_result = self.send_to_socket(message).await;
                if send_result.is_ok() {
                    // Set up to wait for ACK only
                    self.await_response_deadline = Some(
                        tokio::time::Instant::now() + crate::TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL,
                    );
                    self.active_request = Some(ControlMessage::AwaitAck(response));
                } else if let Err(e) = send_result {
                    let _ = response.send(Err(e));
                }
            }
            ControlMessage::ReceiveDiagnosticResponse(timeout, response) => {
                if self.tcp_data_socket.is_none() {
                    if response.send(Err(Error::SocketNotBound)).is_err() {
                        debug!("Failed to send response: Socket not bound");
                    }
                    return;
                }

                // Check if we already have a buffered response from a fast server
                if let Some(buffered) = self.pending_diagnostic_response.take() {
                    debug!("Returning buffered diagnostic response");
                    let _ = response.send(Ok(buffered));
                    return;
                }

                self.await_response_deadline = Some(tokio::time::Instant::now() + timeout);
                // Use a placeholder message - we just want any diagnostic response
                self.active_request = Some(ControlMessage::AwaitResponse(
                    OwnedMessage::default(),
                    response,
                ));
            }
            ControlMessage::AwaitAck(response) => {
                // This is handled in the run loop while receiving messages
                self.active_request = Some(ControlMessage::AwaitAck(response));
            }
        }
    }

    /// Process a message received from the socket. Returns `true` if the run loop should exit.
    async fn process_received_message(&mut self, message: Result<OwnedMessage, Error>) -> bool {
        match message {
            Ok(received_message) => {
                match received_message.payload {
                    Payload::AliveCheckRequest => {
                        trace!("Received Alive Check Request");
                        let _ = self
                            .tcp_data_socket
                            .as_mut()
                            .unwrap()
                            .send(OwnedMessage::alive_check_response(
                                self.client_options.protocol_version,
                                self.client_options.client_logical_address,
                            ))
                            .await;
                    }
                    Payload::DiagnosticMessageAck(ref ack) => {
                        // Handle AwaitAck - complete immediately on ACK
                        if let Some(ControlMessage::AwaitAck(response)) = self.active_request.take()
                        {
                            self.await_response_deadline = None;
                            if ack.ack_code.is_positive_ack() {
                                trace!("Received positive ACK, completing send");
                                let _ = response.send(Ok(()));
                            } else {
                                trace!("Received negative ACK: {:?}", ack.ack_code);
                                let _ =
                                    response.send(Err(Error::DiagnosticMessageNack(ack.ack_code)));
                            }
                            return false;
                        }
                        // For AwaitResponse, continue waiting for DiagnosticMessage
                        if ack.ack_code.is_positive_ack() {
                            trace!("Received Diagnostic Message Ack, waiting for full response");
                            return false;
                        }
                        // Negative ACK for AwaitResponse - fall through to handle as error
                    }
                    _ => trace!("Received message: {received_message:?}"),
                }
                if let Some(active) = self.active_request.take() {
                    if let ControlMessage::AwaitResponse(request_message, response) = active {
                        trace!("Received response for request: {:?}", request_message);
                        trace!("{received_message:?}");
                        if request_message.is_response(received_message.header.payload_type) {
                            debug!("Received expected response, sending to the update channel");
                            if response.send(Ok(received_message)).is_err() {
                                return true;
                            }
                        } else if response
                            .send(Err(Error::UnexpectedMessageType(
                                received_message.header.payload_type,
                            )))
                            .is_err()
                        {
                            return true;
                        }
                    }
                } else {
                    // No active request - check if this is a DiagnosticMessage we should buffer
                    if matches!(received_message.payload, Payload::DiagnosticMessage(_)) {
                        debug!(
                            "Buffering diagnostic response (no active request): {:?}",
                            received_message
                        );
                        self.pending_diagnostic_response = Some(received_message);
                    } else {
                        trace!("No active request, sending received message to update channel");
                        self.run = true;
                        if self.update_sender.send(Ok(received_message)).await.is_err() {
                            tracing::error!("Failed to send received message to update channel");
                            self.run = false;
                            return true;
                        }
                    }
                }
                false
            }
            Err(e) => {
                debug!("Socket error, cleaning up connection: {:?}", e);
                // Clean up the broken socket but keep the inner task running
                // so that reconnection via BindSocket is still possible.
                if let Some(socket) = self.tcp_data_socket.take() {
                    socket.shut_down().await;
                }
                self.await_response_deadline = None;
                self.connection_state = ConnectionState::Listen;
                // Notify any active request of the connection error
                match self.active_request.take() {
                    Some(ControlMessage::AwaitResponse(_, response)) => {
                        let _ = response.send(Err(e));
                    }
                    Some(ControlMessage::AwaitAck(response)) => {
                        let _ = response.send(Err(e));
                    }
                    _ => {}
                }
                false
            }
        }
    }

    /// The run loop receives messages from the socket and handles control messages
    /// (like UDS Requests, Routing Activation) from the outer client.
    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP processing loop");
            loop {
                let mut socket_message: Option<Result<OwnedMessage, Error>> = None;
                let Self {
                    control_receiver,
                    tcp_data_socket,
                    active_request,
                    await_response_deadline,
                    run,
                    ..
                } = &mut self;
                select! {
                    // Only sleep if there is a deadline set
                    () = async {
                        if let Some(deadline) = *await_response_deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }, if active_request.is_some() && await_response_deadline.is_some() => {
                        debug!("Await response deadline reached, server did not respond, which may mean you will not receive Negative Response or message was suppressed");
                        if let Some(ControlMessage::AwaitResponse(_, response)) = active_request.take()
                            && response.send(Err(Error::ResponseTimeoutExceeded)).is_err()
                        {
                            debug!("Failed to send suppressed response");
                        }
                        *await_response_deadline = None;
                    }
                    // Receive a control message
                    ctrl_opt = control_receiver.recv() => {
                        if ctrl_opt.is_none() {
                            debug!("Control channel closed, shutting down inner client");
                            *run = false;
                            break;
                        }
                        assert!(self.active_request.is_none());
                        debug!("Received control message: {:?}", ctrl_opt.as_ref().unwrap());
                        self.active_request = ctrl_opt;
                    }
                    // Receive a message from the socket
                    message = Inner::receive_socket(tcp_data_socket) => {
                        trace!("B: STREAM INCOMING from socket: {message:?}");
                        socket_message = Some(message);
                    }
                }
                if let Some(msg) = socket_message
                    && self.process_received_message(msg).await
                {
                    break;
                }
                self.handle_control_message().await;
            }
        });
    }
}
