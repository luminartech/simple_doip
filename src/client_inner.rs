//! User → Client → control_sender → Inner → SocketManager.sender → TCP Socket → Server
//! User ← Client ← update_receiver ← Inner ← SocketManager.receiver ← TCP Socket ← Server
use std::{future, io::Read};

use tokio::{
    select,
    sync::{mpsc, oneshot},
};

use tracing::{debug, info, trace};
use uds_protocol::WireFormat;

use crate::{client::ClientOptions, connection_state::ConnectionState, Error};
use crate::{messages::*, socket_manager::SocketManager};

/// Messages used to control the DOIP entities
#[derive(Debug)]
pub(super) enum ControlMessage<ReadDefinitions, WriteDefinitions> {
    UDSMessage(
        Message<WriteDefinitions>,
        oneshot::Sender<Result<Message<ReadDefinitions>, Error>>,
    ),
    RoutingActivation(
        RoutingActivationRequest,
        oneshot::Sender<Result<Message<WriteDefinitions>, Error>>,
    ),
    AwaitResponse(
        /// the Request message that was sent
        Message<WriteDefinitions>,
        /// the response channel to send the response to
        oneshot::Sender<Result<Message<ReadDefinitions>, Error>>,
    ),
}

impl<ReadDefinitions: WireFormat, WriteDefinitions: WireFormat + Clone>
    ControlMessage<ReadDefinitions, WriteDefinitions>
{
    pub fn send_routing_activation_request(
        message: &RoutingActivationRequest,
    ) -> (
        oneshot::Receiver<Result<Message<WriteDefinitions>, Error>>,
        Self,
    ) {
        let (sender, receiver) = oneshot::channel();
        (receiver, Self::RoutingActivation(message.clone(), sender))
    }
    /// Takes in a UDS Message Request (WriteDefintions == Request)
    /// Returns:
    /// * a oneshot receiver for the UDS Response (ReadDefinitions == Response)
    /// * a ControlMessage to be sent to the inner client
    pub fn send_request(
        message: &Message<WriteDefinitions>,
    ) -> (
        oneshot::Receiver<Result<Message<ReadDefinitions>, Error>>,
        Self,
    ) {
        let (sender, receiver) = oneshot::channel();
        (receiver, Self::UDSMessage(message.clone(), sender))
    }
}
/// Inner client responsible for the handling of the connection details,
/// including creating 2 channels for sending and receiving messages.
/// it manages its inner state asynchronously, only propagating the
/// results to the outer client when the message is ready.
///
/// The inner state enters an asynchronous loop which runs in the background
///
pub(super) struct Inner<ReadDefinitions, WriteDefinitions> {
    client_options: ClientOptions,
    /// MPSC Receiver used to receive control messages from outer client
    control_receiver: mpsc::Receiver<ControlMessage<ReadDefinitions, WriteDefinitions>>,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<Result<Message<ReadDefinitions>, MessageError>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request: Option<ControlMessage<ReadDefinitions, WriteDefinitions>>,

    /// Socket manager for TCP data socket if bound
    tcp_data_socket: Option<SocketManager<ReadDefinitions, WriteDefinitions>>,

    /// Represents the DoIP connection state of the socket
    connection_state: ConnectionState,

    /// Whether to keep the inner client running
    ///
    /// This is used to gracefully shut down the inner client
    /// when the outer client is dropped
    run: bool,
}

impl<ReadDefinitions, WriteDefinitions> Inner<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync + Clone,
    WriteDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync + Clone,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub fn spawn(
        client_options: ClientOptions,
    ) -> (
        mpsc::Sender<ControlMessage<ReadDefinitions, WriteDefinitions>>,
        mpsc::Receiver<Result<Message<ReadDefinitions>, MessageError>>,
    ) {
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        let inner = Inner {
            client_options,
            control_receiver,
            update_sender,
            active_request: None,
            tcp_data_socket: None,
            connection_state: ConnectionState::Listen,
            run: true,
        };
        inner.run();
        (control_sender, update_receiver)
    }

    async fn bind_socket(&mut self) -> Result<(), Error> {
        // Check if the socket is already bound
        if let Some(_socket) = &self.tcp_data_socket {
            return Ok(());
        } else {
            // Bind the socket
            let socket_manager = SocketManager::bind(self.client_options.clone()).await?;
            self.connection_state = ConnectionState::Initialized;
            self.tcp_data_socket = Some(socket_manager);
            // Send the socket bind message to the control channel
            Ok(())
        }
    }

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

    /// DOIP Routing Activation Request
    async fn request_routing_activation(
        &mut self,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Result<(), Error> {
        if self.connection_state != ConnectionState::Initialized {
            return Err(Error::InvalidConnectionState(
                self.connection_state,
                ConnectionState::Initialized,
            ));
        }
        let message = Message::<WriteDefinitions>::routing_activation_request(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            activation_type,
            reserved_vehicle_manufacturer,
        );
        todo!("Send the message to the server");
        Ok(())
        // Send the message via update_sender
        // self.update_sender
        //     .send(Ok(message))
        //     .await
        //     .map_err(|_| Error::SocketNotBound)
    }

    /// UDS Tester Present AKA Alive check is an inner client only function.
    /// It is used to check if the server is alive and responding without bothering the user with the details
    async fn request_alive_check(&mut self) -> Result<AliveCheckResponse, Error> {
        if self.tcp_data_socket.is_none() {
            return Err(Error::SocketNotBound);
        }
        let header = Header::new(
            self.client_options.protocol_version,
            PayloadType::AliveCheckRequest,
            0,
        );
        let payload = Payload::<WriteDefinitions>::AliveCheckRequest;
        let message = Message { header, payload };

        let (response, message) =
            ControlMessage::<ReadDefinitions, WriteDefinitions>::send_request(&message);

        let response_message = self
            .tcp_data_socket
            .as_mut()
            .unwrap()
            .receive()
            .await
            .unwrap()?;
        if let Payload::AliveCheckResponse(response_payload) = response_message.payload {
            Ok(response_payload)
        } else {
            Err(Error::UnexpectedMessageType(
                response_message.header.payload_type,
            ))
        }
    }

    async fn receive_socket(
        socket_manager: &mut Option<SocketManager<ReadDefinitions, WriteDefinitions>>,
    ) -> Result<Message<ReadDefinitions>, Error> {
        if let Some(receiver) = socket_manager {
            match receiver.receive().await {
                Some(message) => message.map_err(|e| Error::SocketClosedUnexpectedly),
                None => Err(Error::SocketClosedUnexpectedly),
            }
        } else {
            // If we don't have a receiver, we should return a future that never resolves
            future::pending().await
        }
    }

    async fn handle_control_message(&mut self) {
        if let Some(active_request) = self.active_request.take() {
            match active_request {
                ControlMessage::RoutingActivation(request, response) => {
                    // let response = self
                    //     .request_routing_activation(activation_type, reserved_vehicle_manufacturer)
                    //     .await;
                    // response.send()
                    // if let Err(e) = response {
                    //     debug!("Failed to handle routing activation request: {:?}", e);
                    // }
                }
                ControlMessage::UDSMessage(message, response) => {
                    if self.tcp_data_socket.is_none() {
                        if response.send(Err(Error::SocketNotBound)).is_err() {
                            debug!("Failed to send response: Socket not bound");
                            return;
                        }
                    } else {
                        let send_result = self
                            .tcp_data_socket
                            .as_mut()
                            .unwrap()
                            .send(message.clone())
                            .await;
                        match send_result {
                            Ok(_) => {
                                self.active_request = Some(ControlMessage::AwaitResponse(
                                    message.to_owned(),
                                    response,
                                ));
                            }
                            Err(_) => todo!(),
                        }
                        // Handle UDS message
                        debug!("Handling UDS message: {:?}", message);
                    }
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
            info!("Starting DOIP processing loop");
            loop {
                let Self {
                    control_receiver,
                    update_sender,
                    tcp_data_socket,
                    active_request,
                    run,
                    ..
                } = &mut self;

                // Read a message from the read stream
                select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(125)) => {}
                    // Receive a control message
                    ctrl = control_receiver.recv() => {
                        if let Some(ctrl) = ctrl {
                            // We should never have an active request already
                            // But maybe we should gracefully handle this
                            // and just ignore the new request?
                            assert!(self.active_request.is_none());
                            debug!("Received control message: {:?}", ctrl);
                            self.active_request = Some(ctrl);
                        } else {
                            // The sender has been dropped, so we should exit
                            break;
                        }
                    }
                    // Receive a message from the socket
                    message = Inner::receive_socket(tcp_data_socket) => {
                        trace!("Received message from socket: {:?}", message);
                        match message {
                            Ok(received_message) => {
                                if let Some(active) = active_request.take() {
                                    // If the active request is an AwaitResponse that matches the received message,
                                    // send the response to the update channel
                                    if let ControlMessage::AwaitResponse(request_message, response) = active {
                                        trace!("Received response for request: {:?}", request_message);
                                        if received_message.header.payload_type == request_message.header.payload_type {
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
                        }
                    }
                    }

                }
                self.handle_control_message().await;
            }
        });
    }
}
