use crate::client_inner::ControlMessage;
use crate::connection;
use crate::{
    client_inner::Inner,
    messages::{ActivationTypeCode, Message, MessageError, ProtocolVersion},
    traits::WirePayload,
    Error, LogicalAddress,
};
use std::net::{IpAddr, SocketAddr};
use tokio::sync::mpsc;
use tracing::{info, trace};

#[derive(Debug, strum::Display)]
/// Send updates to the user
pub enum ClientUpdate<ReadDefinitions> {
    /// Unicase message from the server
    Unicast(Message<ReadDefinitions>),
    /// Inner DoIP client error
    Error(Error),
}

/// Activation options for the routing activation request
///
/// This is used to determine which type of routing activation request to send
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingActivationOptions {
    /// Activation type code
    pub activation_type: ActivationTypeCode,
    /// OEM specific data
    pub oem_specific: Option<[u8; 4]>,
}

/// DoIP client options used to specify connection info
/// Derive `Serialize` and `Deserialize` for use in config files
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientOptions {
    /// Server IP address and port
    pub server_address: SocketAddr,
    /// Target logical addresses, uniquely identifies the ECU to be diagnosed.
    /// Valid range: 0x0001 - 0x0DFF
    pub server_logical_address: LogicalAddress,
    /// (Logical address) Valid range: 0x0001 - 0x0DFF
    pub server_physical_address: LogicalAddress,
    /// Local ip address to bind the TCP and UDP sockets to, e.g. `0.0.0.0`. The port is randomly chosen.
    pub client_address: IpAddr,
    /// Valid range: 0x0E00 - 0x0FFF
    pub client_logical_address: LogicalAddress,
    /// Which protocol version the client should
    pub protocol_version: ProtocolVersion,
    /// The activation type to use when sending the routing activation request
    pub routing_activation_options: Option<RoutingActivationOptions>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressType {
    Logical,
    Physical,
}

/// The client is the main entry point for the user to interact with the DoIP protocol.
///
/// It handles the connection to the server, and sends and receives messages, silently
/// handling DoIP acknowledgements and other protocol details that the user doesn't need to worry about.
#[derive(Debug)]
pub struct Client<ReadDefinitions, WriteDefinitions, Conn = connection::ConnectorSocket> {
    pub client_options: ClientOptions,
    /// Sends messages from the user to the inner client
    control_sender: mpsc::Sender<ControlMessage<ReadDefinitions, WriteDefinitions>>,
    /// Receives messages from the inner client to the user
    update_receiver: mpsc::Receiver<Result<Message<ReadDefinitions>, MessageError>>,
    _phantom: std::marker::PhantomData<Conn>,
}

impl<
        ReadDefinitions: WirePayload + 'static + Sync + Send + Clone,
        WriteDefinitions: WirePayload + 'static + Sync + Send + Clone,
        Conn: connection::Connector + 'static + Sync + Send,
    > Client<ReadDefinitions, WriteDefinitions, Conn>
{
    /// Create a DoIP connection, and automatically send a routing activation request if the client options specify it
    /// The target port defaults to [`crate::TCP_PORT`].
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        let (control_sender, update_receiver) = Inner::<_, _, Conn>::spawn(client_options);
        // Automatically send a routing activation request if the client options specify it
        if let Some(routing_activation_options) = client_options.routing_activation_options {
            let message = Message::<WriteDefinitions>::routing_activation_request(
                client_options.protocol_version,
                client_options.client_logical_address,
                routing_activation_options.activation_type,
                routing_activation_options.oem_specific,
            );

            // Send the message and wait for a response
            let (response, message) = ControlMessage::create_message(&message);
            control_sender.send(message).await.unwrap();
            let _ = response.await;
        }

        Ok(Self {
            client_options,
            control_sender,
            update_receiver,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Bind the socket to a local address and port.
    ///
    /// * Standard ISO-13400 clients will bind to the local address and port of the server
    /// * See [Inner::bind_socket] for more details
    pub async fn bind_socket(&mut self, gateway_address: SocketAddr) -> Result<u16, Error> {
        let (response, message) = ControlMessage::create_bind_socket_message(gateway_address);
        self.control_sender
            .send(message)
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        response.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// Unbind the socket from the local address and port.
    pub async fn unbind_socket(&mut self) -> Result<(), Error> {
        let (response, message) = ControlMessage::create_unbind_socket_message();
        self.control_sender.send(message).await.unwrap();
        response.await.unwrap()
    }

    /// Returns an Option of a Response if there was one in flight when the client or server disconnected
    pub async fn reconnect(&mut self) -> Result<Option<Message<WriteDefinitions>>, Error> {
        todo!("Reconnect not implemented yet");
    }

    /// Automatically send UDS tester present's to the server
    /// This is useful for keeping the connection alive as a set and forget
    pub async fn auto_tester_present(send_tester_present: bool) {
        trace!("Auto tester present: {}", send_tester_present);
        todo!("Auto tester present not implemented yet");
    }

    /// Send a UDS message to the server
    ///
    /// This is a generic message that can be used to send any UDS message
    pub async fn send_diagnostic_message(
        &mut self,
        address_type: AddressType,
        user_data: WriteDefinitions,
    ) -> Result<Message<ReadDefinitions>, Error> {
        // Create a new message, send it to the server, and wait for a response

        // Create a new message
        let message = Message::<WriteDefinitions>::diagnostic_message(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            match address_type {
                AddressType::Logical => self.client_options.server_logical_address,
                AddressType::Physical => self.client_options.server_physical_address,
            },
            user_data,
        );

        // Send the message and wait for a response
        self.send_message(&message).await
    }

    /// Send a request to the server and wait for a response (which is returned)
    async fn send_message(
        &mut self,
        control: &Message<WriteDefinitions>,
    ) -> Result<Message<ReadDefinitions>, Error> {
        // Create new request and response channels to await the response of
        let (response, message) = ControlMessage::create_message(control);
        self.control_sender.send(message).await.unwrap();
        response.await.unwrap()
    }

    /// Shut down the client, closing the connection and cleaning up resources
    pub async fn shut_down(self) {
        let Self {
            control_sender,
            mut update_receiver,
            ..
        } = self;
        drop(control_sender);
        info!("Shutting Down DOIP client");
        while update_receiver.recv().await.is_some() {
            info!(".");
        }
    }
}
