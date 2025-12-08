//! User → Client → control_sender → Inner → SocketManager.sender → TCP Socket → Server
//! User ← Client ← update_receiver ← Inner ← SocketManager.receiver ← TCP Socket ← Server
use crate::{
    client::{ClientOptions, SendResult},
    connection_state::ConnectionState,
    messages::*,
    socket_manager::SocketManager,
    Error,
};
use std::{future, net::SocketAddr};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tracing::{debug, trace};


/// Messages used to control the DOIP entities
#[allow(unused)]
#[derive(Debug)]
pub(super) enum ControlMessage {
    /// No payload
    AliveCheckRequest(oneshot::Sender<Result<(), Error>>),
    AliveCheckResponse(Message),
    /// No oneshot needed, the response is sent
    /// and does not need to be awaited
    SendNoResponse(Message),

    BindSocket(SocketAddr, oneshot::Sender<Result<u16, Error>>),
    UnbindSocket(oneshot::Sender<Result<(), Error>>),
    UDSMessage(
        Message,
        oneshot::Sender<Result<SendResult<Message>, Error>>,
    ),
    RoutingActivation(
        Message,
        oneshot::Sender<Result<Message, Error>>,
    ),
    AwaitResponse(
        /// the Request message that was sent
        Message,
        /// the response channel to send the response to
        oneshot::Sender<Result<Message, Error>>,
    ),
}

/// Result type for the `create_message` function
///
/// Contains a oneshot receiver for the response and the control message to be sent
/// to the inner client.
pub(crate) struct CreateMessageResult(
    pub oneshot::Receiver<Result<SendResult<Message>, Error>>,
    pub ControlMessage,
);

impl ControlMessage {
    /// Helper method to create control messages with oneshot channels
    fn create_oneshot<T>(
        factory: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Self,
    ) -> (oneshot::Receiver<Result<T, Error>>, Self) {
        let (sender, receiver) = oneshot::channel();
        (receiver, factory(sender))
    }

    #[allow(unused)]
    pub fn create_routing_activation_message(
        message: &Message,
    ) -> (
        oneshot::Receiver<Result<Message, Error>>,
        Self,
    ) {
        trace!(message = "RoutingActivationRequest");
        Self::create_oneshot(|sender| Self::RoutingActivation(message.clone(), sender))
    }

    /// Takes in a UDS Message Request (WriteDefinitions == Request)
    /// Returns:
    /// * a oneshot receiver for the UDS Response (ReadDefinitions == Response)
    /// * a ControlMessage to be sent to the inner client
    pub fn create_message(
        message: Message,
    ) -> CreateMessageResult {
        let (rx, msg) = Self::create_oneshot(|sender| Self::UDSMessage(message, sender));
        CreateMessageResult(rx, msg)
    }

    /// Builder for the bind socket message
    pub fn create_bind_socket_message(
        gateway_address: SocketAddr,
    ) -> (oneshot::Receiver<Result<u16, Error>>, Self) {
        Self::create_oneshot(|sender| Self::BindSocket(gateway_address, sender))
    }

    pub fn create_unbind_socket_message() -> (oneshot::Receiver<Result<(), Error>>, Self) {
        Self::create_oneshot(Self::UnbindSocket)
    }
}

/// Inner client responsible for the handling of the connection details,
/// including creating 2 channels for sending and receiving messages.
/// it manages its inner state asynchronously, only propagating the
/// results to the outer client when the message is ready.
///
/// The inner state enters an asynchronous loop which runs in the background
///
pub(super) struct Inner<Conn> {
    client_options: ClientOptions,
    /// MPSC Receiver used to receive control messages from outer client
    control_receiver: mpsc::Receiver<ControlMessage>,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<Result<Message, MessageError>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request: Option<ControlMessage>,

    /// Deadline for awaiting a response
    await_response_deadline: Option<tokio::time::Instant>,

    /// Socket manager for TCP data socket if bound
    tcp_data_socket: Option<SocketManager<Conn>>,

    /// Represents the DoIP connection state of the socket
    connection_state: ConnectionState,

    /// Whether to keep the inner client running
    ///
    /// This is used to gracefully shut down the inner client
    /// when the outer client is dropped
    run: bool,

    // tester_present_heartbeat: tokio::time::Interval,
    last_tester_present: tokio::time::Instant,

    /// Tester Present request message to be sent
    /// This is used to send the Tester Present message periodically
    tester_present_message: Message,
}
/// Sender for the control messages sent via the control channel
type ControlSender = mpsc::Sender<ControlMessage>;
/// Receiver for the update messages from the socket
type UpdateReceiver<E> = mpsc::Receiver<Result<Message, E>>;

impl<Conn> Inner<Conn>
where
    Conn: crate::connection::Connector + 'static + Send + Sync,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub fn spawn(
        client_options: ClientOptions,
    ) -> (
        ControlSender,
        UpdateReceiver<MessageError>,
    ) {
        trace!("Spawning inner client");
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        // Create a simple tester present message as bytes
        // This is a basic UDS tester present: [0x3E, 0x80] (service 0x3E with suppress positive response)
        let tester_present_bytes = if client_options.suppress_tester_present {
            vec![0x3E, 0x80]  // Tester Present with suppress positive response
        } else {
            vec![0x3E, 0x00]  // Tester Present without suppress positive response
        };
        let tester_present_message = Message::diagnostic_message(
            client_options.protocol_version,
            client_options.client_logical_address,
            client_options.server_physical_address,
            tester_present_bytes,
        );
        let inner = Inner::<Conn> {
            client_options,
            control_receiver,
            update_sender,
            active_request: None,
            await_response_deadline: None,
            tcp_data_socket: None,
            connection_state: ConnectionState::Listen,
            run: true,
            last_tester_present: tokio::time::Instant::now(),
            tester_present_message,
        };
        inner.run();
        (control_sender, update_receiver)
    }

    /// Binds the unicast socket to the specified address
    async fn bind_socket(&mut self, gateway_address: SocketAddr) -> Result<u16, Error> {
        // Check if the socket is already bound
        if let Some(socket) = &self.tcp_data_socket {
            Ok(socket.port())
        } else {
            // Bind the socket
            let socket_manager: SocketManager<Conn> = SocketManager::bind(self.client_options, gateway_address).await?;
            self.connection_state = ConnectionState::Initialized;
            let port = socket_manager.port();
            debug!("Bound socket to port: {}", port);
            self.tcp_data_socket = Some(socket_manager);
            self.run = true;
            // Send the socket bind message to the control channel
            Ok(port)
        }
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
        self.send_to_socket(Message::alive_check_response(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
        ))
        .await
    }

    /// Send a message to the socket manager
    ///
    /// Also keeps track of the last time a message was sent to the socket for Tester Present purposes
    ///
    /// Errors:
    /// [Error::SocketNotBound] - if the socket is not bound
    async fn send_to_socket(
        &mut self,
        message: Message,
    ) -> Result<(), Error> {
        if let Some(socket) = &mut self.tcp_data_socket {
            self.last_tester_present = tokio::time::Instant::now();
            socket.send(message).await
        } else {
            Err(Error::SocketNotBound)
        }
    }

    async fn receive_socket(
        socket_manager: &mut Option<SocketManager<Conn>>,
    ) -> Result<Message, Error> {
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

    /// Handle the [Inner::active_request] that was set in the [Inner::run] loop.
    ///
    /// The `response` is a oneshot channel that is used to (generally) send the response back to the facade client
    ///
    /// ### Diagnostic Acks:
    /// * will not be sent to the user
    /// * will be handled internally
    async fn handle_control_message(&mut self) {
        // No active request? Nothing to do
        let Some(control_message) = self.active_request.take() else {
            return;
        };

        match control_message {
            ControlMessage::AliveCheckRequest(response) => {
                let send_result = self
                    .send_to_socket(Message::alive_check_request(
                        self.client_options.protocol_version,
                    ))
                    .await;
                if response.send(send_result).is_err() {
                    debug!("Failed to send alive check response");
                }
            }
            ControlMessage::AliveCheckResponse(message) => {
                let _ = self
                    .send_to_socket(Message::alive_check_response(
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
                match send_result {
                    Ok(_) => {
                        self.await_response_deadline = Some(
                            tokio::time::Instant::now() + crate::TCP_TIMEOUT_INITIAL_INACTIVITY,
                        );
                        self.active_request =
                            Some(ControlMessage::AwaitResponse(message.to_owned(), response))
                    }
                    Err(_) => todo!(),
                };
            }
            ControlMessage::UDSMessage(message, response) => {
                if self.tcp_data_socket.is_none() {
                    if response.send(Err(Error::SocketNotBound)).is_err() {
                        debug!("Failed to send response: Socket not bound");
                    }
                    return;
                }
                let send_result = self.send_to_socket(message.clone()).await;

                // Wait for the initial diagnostic message timeout before considering the message suppressed or timed out
                tokio::time::sleep_until(
                    self.last_tester_present + crate::TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL,
                )
                .await;
                // Since DoIP is now transport-agnostic, we can't determine
                // if a response is suppressed from the opaque payload.
                // This logic should be handled at the application (UDS) layer.
                let was_suppressed = false;  // Always false at transport layer
                match send_result {
                    Ok(_) => {
                        let (await_sender, await_receiver) = oneshot::channel();

                        self.active_request = Some(ControlMessage::AwaitResponse(
                            message.to_owned(),
                            await_sender,
                        ));
                        // Converts from the SendResult return to a regular AwaitResponse
                        // and spawns a task to await the response
                        tokio::spawn(async move {
                            match await_receiver.await {
                                Ok(Ok(response_message)) => {
                                    // If we received a response with a
                                    if response
                                        .send(Ok(SendResult::Response(response_message)))
                                        .is_err()
                                    {
                                        debug!("Failed to send response");
                                    }
                                }
                                Ok(Err(Error::ResponseTimeoutExceeded)) => {
                                    if was_suppressed {
                                        // If the message was suppressed, we should not send an error
                                        if response.send(Ok(SendResult::Suppressed)).is_err() {
                                            debug!(
                                                "Failed to send suppressed response after timeout"
                                            );
                                        }
                                    } else {
                                        // Let the caller know that the response timed out
                                        if response
                                            .send(Err(Error::ResponseTimeoutExceeded))
                                            .is_err()
                                        {
                                            debug!("Failed to send timeout error response");
                                        }
                                    }
                                }
                                Ok(Err(e)) => {
                                    if response.send(Err(e)).is_err() {
                                        debug!("Failed to send error response");
                                    }
                                }
                                Err(_) => {
                                    debug!("Failed to receive response");
                                    let _ = response.send(Err(Error::ConnectionClosed));
                                    // If the receiver was dropped, we should exit
                                }
                            }
                        });
                    }
                    Err(_) => {
                        debug!("Failed to send UDS message");
                        if response.send(Err(Error::SocketNotBound)).is_err() {
                            debug!("Failed to send response: Socket not bound");
                        }
                    }
                }
                // Handle UDS message
                debug!("Handling UDS message: {:?}", message);
            }
            ControlMessage::AwaitResponse(message, response) => {
                trace!("Awaiting response for message: {:?}", message);
                // This is handled in the run loop while receiving messages
                self.active_request = Some(ControlMessage::AwaitResponse(message, response));
            }
        }
    }

    /// The run loop will handle sending Tester Present messages, receives messages from the socket,
    /// and handles control messages (like UDS Requests, Routing Activation) from the outer client.
    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP processing loop");
            loop {
                let Self {
                    control_receiver,
                    update_sender,
                    tcp_data_socket,
                    active_request,
                    await_response_deadline,
                    client_options,
                    run,
                    last_tester_present,
                    tester_present_message,
                    ..
                } = &mut self;
                select! {
                    // Only sleep if there is a deadline set
                    _ = async {
                        if let Some(deadline) = *await_response_deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }, if active_request.is_some() && await_response_deadline.is_some() => {
                        debug!("Await response deadline reached, server did not respond, which may mean you will not receive Negative Response or message was suppressed");
                        if let Some(ControlMessage::AwaitResponse(_, response)) = active_request.take() {
                            if response.send(Err(Error::ResponseTimeoutExceeded)).is_err() {
                                debug!("Failed to send suppressed response");
                            }
                        }
                        *await_response_deadline = None;
                    }
                    // Handle the UDS TesterPresent heartbeat
                    _ = tokio::time::sleep_until(*last_tester_present + client_options.tester_present_interval), if *run => {
                        let Some(socket_manager) = tcp_data_socket.as_mut() else {
                            debug!("No socket manager available, skipping tester present message");
                            continue;
                        };
                        if last_tester_present.elapsed() < client_options.tester_present_interval {
                            continue;
                        }
                        if let Err(e) = socket_manager.send(tester_present_message.clone()).await {
                            debug!("Failed to send tester present message: {:?}", e);
                        } else {
                            *last_tester_present = tokio::time::Instant::now();
                        }
                    }
                    // Receive a control message
                    ctrl_opt = control_receiver.recv() => {
                        // We should never have an active request already
                        // But maybe we should gracefully handle this
                        // and just ignore the new request?
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
                        *last_tester_present = tokio::time::Instant::now();
                        trace!("B: STREAM INCOMING from socket: {message:?}");
                        match message {
                            Ok(received_message) => {
                                match received_message.payload {
                                    Payload::AliveCheckRequest => {
                                        trace!("Received Alive Check Request");
                                        // Send the alive check response automatically
                                        let _ = tcp_data_socket
                                            .as_mut()
                                            .unwrap()
                                            .send(Message::alive_check_response(
                                                client_options.protocol_version,
                                                client_options.client_logical_address,
                                            ))
                                            .await;
                                    }
                                    Payload::DiagnosticMessageAck(_) => {
                                        trace!("Received Diagnostic Message Ack, waiting for full response");
                                    }
                                    _ => {
                                        trace!("Received message: {received_message:?}");
                                    }
                                }
                                if let Some(active) = active_request.take() {
                                    // If the active request is an AwaitResponse that matches the received message,
                                    // send the response to the update channel
                                    if let ControlMessage::AwaitResponse(request_message, response) = active {
                                        trace!("Received response for request: {:?}", request_message);
                                        trace!("{received_message:?}");
                                        if request_message.is_response(received_message.header.payload_type) {
                                            debug!("Received expected response, sending to the update channel");
                                        // if received_message.header.payload_type == request_message.header.payload_type {
                                            if response.send(Ok(received_message)).is_err() {
                                                // The receiver has been dropped, so we should exit
                                                break;
                                            }
                                        } else {
                                            // If the message is not the expected response, send an error
                                            if response.send(Err(Error::UnexpectedMessageType(received_message.header.payload_type))).is_err() {
                                                // The receiver has been dropped, so we should exit
                                                break;
                                            }
                                        }
                                    }
                                } else {
                                    trace!("No active request, sending received message to update channel");
                                    *run = true;
                                    // Resend the received message to the update channel
                                    // TODO: Check that the update_sender is correct??
                                    if update_sender.send(Ok(received_message)).await.is_err() {
                                        tracing::error!("Failed to send received message to update channel");
                                        *run = false;
                                        // The receiver has been dropped, so we should exit
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("Error receiving message from socket: {:?}", e);
                                // Handle the error
                                break;
                            }
                        }
                    }
                }
                self.handle_control_message().await;
            }
        });
    }
}
