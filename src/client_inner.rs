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
use uds_protocol::{DiagnosticDefinition, WireFormat};

/// Messages used to control the DOIP entities
#[allow(unused)]
#[derive(Debug)]
pub(super) enum ControlMessage<ReadDefinitions, WriteDefinitions> {
    /// No payload
    AliveCheckRequest(oneshot::Sender<Result<(), Error>>),
    AliveCheckResponse(Message<ReadDefinitions>),
    /// No oneshot needed, the response is sent
    /// and does not need to be awaited
    SendNoResponse(Message<WriteDefinitions>),

    BindSocket(SocketAddr, oneshot::Sender<Result<u16, Error>>),
    UnbindSocket(oneshot::Sender<Result<(), Error>>),
    UDSMessage(
        Message<WriteDefinitions>,
        oneshot::Sender<Result<SendResult<Message<ReadDefinitions>>, Error>>,
    ),
    RoutingActivation(
        Message<WriteDefinitions>,
        oneshot::Sender<Result<Message<ReadDefinitions>, Error>>,
    ),
    AwaitResponse(
        /// the Request message that was sent
        Message<WriteDefinitions>,
        /// the response channel to send the response to
        oneshot::Sender<Result<Message<ReadDefinitions>, Error>>,
    ),
}

/// Result type for the `create_message` function
///
/// Contains a oneshot receiver for the response and the control message to be sent
/// to the inner client.
pub(crate) struct CreateMessageResult<ReadDefinitions, WriteDefinitions>(
    pub oneshot::Receiver<Result<SendResult<Message<ReadDefinitions>>, Error>>,
    pub ControlMessage<ReadDefinitions, WriteDefinitions>,
);

impl<ReadDefinitions, WriteDefinitions> ControlMessage<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat,
    WriteDefinitions: WireFormat + Clone,
{
    /// Helper method to create control messages with oneshot channels
    fn create_oneshot<T>(
        factory: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Self,
    ) -> (oneshot::Receiver<Result<T, Error>>, Self) {
        let (sender, receiver) = oneshot::channel();
        (receiver, factory(sender))
    }

    #[allow(unused)]
    pub fn create_routing_activation_message(
        message: &Message<WriteDefinitions>,
    ) -> (
        oneshot::Receiver<Result<Message<ReadDefinitions>, Error>>,
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
        message: Message<WriteDefinitions>,
    ) -> CreateMessageResult<ReadDefinitions, WriteDefinitions> {
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
pub(super) struct Inner<DiagTypes: DiagnosticDefinition, Conn> {
    client_options: ClientOptions,
    /// MPSC Receiver used to receive control messages from outer client
    control_receiver: mpsc::Receiver<
        ControlMessage<uds_protocol::Response<DiagTypes>, uds_protocol::Request<DiagTypes>>,
    >,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<Result<Message<uds_protocol::Response<DiagTypes>>, MessageError>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request:
        Option<ControlMessage<uds_protocol::Response<DiagTypes>, uds_protocol::Request<DiagTypes>>>,

    /// Socket manager for TCP data socket if bound
    tcp_data_socket: Option<SocketManager<DiagTypes, Conn>>,

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
    tester_present_message: Message<uds_protocol::Request<DiagTypes>>,
}
/// Sender for the control messages sent via the control channel
type ControlSender<DiagTypes> = mpsc::Sender<
    ControlMessage<uds_protocol::Response<DiagTypes>, uds_protocol::Request<DiagTypes>>,
>;
/// Receiver for the update messages from the socket
type UpdateReceiver<DiagTypes, E> =
    mpsc::Receiver<Result<Message<uds_protocol::Response<DiagTypes>>, E>>;

impl<DiagTypes, Conn> Inner<DiagTypes, Conn>
where
    DiagTypes: DiagnosticDefinition + std::fmt::Debug + 'static + Send + Sync + Clone,
    Conn: crate::connection::Connector + 'static + Send + Sync,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub fn spawn(
        client_options: ClientOptions,
    ) -> (
        ControlSender<DiagTypes>,
        UpdateReceiver<DiagTypes, MessageError>,
    ) {
        trace!("Spawning inner client");
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        let tester_present_message = Message::diagnostic_message(
            client_options.protocol_version,
            client_options.client_logical_address,
            client_options.server_physical_address,
            uds_protocol::Request::tester_present(client_options.suppress_tester_present),
        );
        let inner = Inner::<DiagTypes, Conn> {
            client_options,
            control_receiver,
            update_sender,
            active_request: None,
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
            let socket_manager = SocketManager::bind(self.client_options, gateway_address).await?;
            self.connection_state = ConnectionState::Initialized;
            let port = socket_manager.port();
            debug!("Bound socket to port: {}", port);
            self.tcp_data_socket = Some(socket_manager);
            // Send the socket bind message to the control channel
            Ok(port)
        }
    }

    /// Unbind the socket
    /// This is used to gracefully shut down the socket manager
    async fn unbind_socket(&mut self) -> Result<(), Error> {
        // Check if the socket is already bound
        if let Some(socket_manager) = self.tcp_data_socket.take() {
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
    async fn send_to_socket(
        &mut self,
        message: Message<uds_protocol::Request<DiagTypes>>,
    ) -> Result<(), Error> {
        if let Some(socket) = &mut self.tcp_data_socket {
            self.last_tester_present = tokio::time::Instant::now();
            socket.send(message).await
        } else {
            Err(Error::SocketNotBound)
        }
    }

    async fn receive_socket(
        socket_manager: &mut Option<SocketManager<DiagTypes, Conn>>,
    ) -> Result<Message<uds_protocol::Response<DiagTypes>>, Error> {
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
        if let Some(active_request) = self.active_request.take() {
            match active_request {
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

                    // Check for a successful suppressed message, if it is, send a Suppressed response
                    // If a RequestCorrectlyRecieved-ResponsePending NRC is received, the server WILL send a response
                    // so we should not send a Suppressed response
                    if message.is_positive_response_suppressed() && send_result.is_ok() {
                        if response.send(Ok(SendResult::Suppressed)).is_err() {
                            debug!("Failed to send suppressed response");
                        }
                        return;
                    }
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
                        Err(_) => todo!(),
                    }
                    // Handle UDS message
                    debug!("Handling UDS message: {:?}", message);
                }
                ControlMessage::AwaitResponse(message, response) => {
                    // This is handled in the run loop while receiving messages
                    self.active_request = Some(ControlMessage::AwaitResponse(message, response));
                }
            }
        }
    }

    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP processing loop");
            loop {
                let Self {
                    control_receiver,
                    update_sender,
                    tcp_data_socket,
                    active_request,
                    client_options,
                    run,
                    last_tester_present,
                    tester_present_message,
                    ..
                } = &mut self;
                select! {
                    // Handle the UDS TesterPresent heartbeat
                    _ = tokio::time::sleep_until(*last_tester_present + client_options.tester_present_interval) => {
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
                        debug!("Received control message: {:?}", ctrl_opt);
                        self.active_request = ctrl_opt;
                    }
                    // Receive a message from the socket
                    message = Inner::receive_socket(tcp_data_socket) => {
                        *last_tester_present = tokio::time::Instant::now();
                        trace!("Received message from socket: {:?}", message);
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
                                        trace!("Received message: {:?}", received_message);
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
                                    // Resend the received message to the update channel
                                    // TODO: Check that the update_sender is correct??
                                    if update_sender.send(Ok(received_message)).await.is_err() {
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
