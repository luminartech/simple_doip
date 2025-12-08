use crate::{
    client_inner::{ControlMessage, CreateMessageResult, Inner},
    connection,
    messages::{
        ActivationTypeCode, Message, MessageError, ProtocolVersion, RoutingActivationResponse,
    },
    Error, LogicalAddress, TCP_TIMEOUT_INITIAL_INACTIVITY,
};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::{debug, info, trace};


#[derive(Debug, strum::Display)]
/// Send updates to the user
pub enum ClientUpdate {
    /// Unicase message from the server
    Unicast(Message),
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
    /// Timer for sending the UDS Tester Present messages
    /// Not necessary for DefaultSession, but useful for keeping the connection alive
    pub tester_present_interval: tokio::time::Duration,

    /// Whether to suppress the UDS TesterPresent reply from the server
    pub suppress_tester_present: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressType {
    Logical,
    Physical,
}

/// The result of sending a message to the server. When a message
/// is suppressed (ie via UDS), the server might not respond and returns `Suppressed`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendResult<ReadDefinitions> {
    /// The message was sent successfully
    Response(ReadDefinitions),
    /// The message was sent, but the server did not respond
    Suppressed,
}

/// The client is the main entry point for the user to interact with the DoIP protocol.
///
/// It handles the connection to the server, and sends and receives messages, silently
/// handling DoIP acknowledgements and other protocol details that the user doesn't need to worry about.
#[derive(Debug)]
pub struct Client<Conn = connection::ConnectorSocket> {
    pub client_options: ClientOptions,
    /// Sends messages from the user to the inner client
    control_sender: mpsc::Sender<ControlMessage>,
    /// Receives messages from the inner client to the user
    update_receiver:
        mpsc::Receiver<Result<Message, MessageError>>,
    _phantom: std::marker::PhantomData<Conn>,
}

impl<Conn> Client<Conn>
where
    Conn: connection::Connector + 'static + Sync + Send,
{
    /// Create a DoIP connection, and automatically send a routing activation request if the client options specify it
    /// The target port defaults to [`crate::TCP_PORT`].
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        let (control_sender, update_receiver) = Inner::<Conn>::spawn(client_options);
        Self::bind_socket(&control_sender, &client_options).await?;

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
    async fn bind_socket(
        control_sender: &mpsc::Sender<ControlMessage>,
        client_options: &ClientOptions,
    ) -> Result<u16, Error> {
        let (response, message) =
            ControlMessage::create_bind_socket_message(client_options.server_address);
        control_sender.send(message).await.map_err(|_| {
            Error::BindFailed("Could not send BindSocket message to inner client".into())
        })?;
        let port = response.await.map_err(|_| Error::ConnectionClosed)?;

        // Automatically send a routing activation request if the client options specify it
        'routing: {
            if let Some(routing_activation_options) = client_options.routing_activation_options {
                let message = Message::routing_activation_request(
                    client_options.protocol_version,
                    client_options.client_logical_address,
                    routing_activation_options.activation_type,
                    routing_activation_options.oem_specific,
                );

                // Send the message and wait for a response
                let (response, message) =
                    ControlMessage::create_routing_activation_message(&message);
                control_sender
                    .send(message)
                    .await
                    .map_err(|_| Error::RoutingActivationFailed)
                    .inspect_err(|e| debug!("Failed to send routing activation request: {e}"))
                    .inspect(|_| trace!("Routing activation request sent successfully"))?;
                let res = tokio::time::timeout(TCP_TIMEOUT_INITIAL_INACTIVITY, response).await;
                // Elapsed error handling
                let Ok(res) = res else {
                    tracing::warn!("Timeout waiting for routing activation response. Server may not support routing activation.");
                    break 'routing;
                };
                let Ok(res) = res else {
                    tracing::warn!("Routing activation response channel closed. Server may not support routing activation.");
                    break 'routing;
                };
                // if the timeout specifically and keep working, the routing activation may not be supported
                debug!("Routing Activation Response received: {:?}", res);
                match res {
                    Ok(Message { payload, header: _ }) => {
                        let RoutingActivationResponse {
                            logical_address_tester,
                            logical_address_of_doip_entity,
                            routing_activation_response_code,
                            reserved_oem,
                            oem_specific,
                        } = match payload {
                            crate::messages::Payload::RoutingActivationResponse(resp) => resp,
                            _ => {
                                todo!(
                                "Responded with something other than a routing activation response"
                            );
                                // return Err(Error::MessageError(MessageError::InvalidPayloadType));
                            }
                        };
                        info!("Routing Activation Response received:");
                        info!("  Logical Address Tester: {:04X}", logical_address_tester.0);
                        info!(
                            "  Logical Address of DoIP Entity: {:04X}",
                            logical_address_of_doip_entity.0
                        );
                        info!(
                            "  Routing Activation Response Code: {:?}",
                            routing_activation_response_code
                        );
                        info!("  Reserved OEM: {:02X?}", reserved_oem);
                        if let Some(oem) = oem_specific {
                            info!("  OEM Specific: {:02X?}", oem);
                        }
                    }
                    Err(_) => {
                        return Err(Error::ConnectionClosed);
                    }
                }
            }
        }
        port
    }

    /// Unbind the socket from the local address and port.
    pub async fn unbind_socket(&mut self) -> Result<(), Error> {
        let (response, message) = ControlMessage::create_unbind_socket_message();
        self.control_sender.send(message).await.unwrap();
        response.await.unwrap()
    }

    /// Returns an Option of a Response if there was one in flight when the client or server disconnected
    pub async fn reconnect(
        &mut self,
    ) -> Result<Option<Message>, Error> {
        let _ = Self::bind_socket(&self.control_sender, &self.client_options).await?;
        trace!("Reconnected, checking for in-flight messages over 5 seconds");
        let res =
            tokio::time::timeout(Duration::from_millis(5000), self.update_receiver.recv()).await;
        // Elapsed error handling, no response in flight
        let Ok(res) = res else {
            return Ok(None);
        };
        let Some(msg_res) = res else {
            return Ok(None);
        };
        let Ok(msg) = msg_res else { return Ok(None) };
        debug!("Reconnected, received in-flight message: {:?}", msg);
        Ok(Some(msg))
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
        user_data: Vec<u8>,
    ) -> Result<SendResult<Message>, Error> {
        // Create a new message, send it to the server, and wait for a response

        // Create a new message
        let message = Message::diagnostic_message(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            match address_type {
                AddressType::Logical => self.client_options.server_logical_address,
                AddressType::Physical => self.client_options.server_physical_address,
            },
            user_data,
        );

        // Send the message and wait for a response
        self.send_message(message).await
    }

    /// Send a request to the server and wait for a response (which is returned)
    async fn send_message(
        &mut self,
        control: Message,
    ) -> Result<SendResult<Message>, Error> {
        // Create new request and response channels to await the response of
        let CreateMessageResult(response, message) = ControlMessage::create_message(control);
        // Sends to Inner client
        if let Err(e) = self.control_sender.send(message).await {
            return Err(Error::SendError(e.to_string()));
        }
        // if its a RecvError the sender has been dropped
        response.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// Shut down the client, closing the connection and cleaning up resources
    pub async fn shut_down(self) {
        let Self {
            control_sender,
            mut update_receiver,
            ..
        } = self;
        drop(control_sender);
        debug!("Shutting Down DOIP client");
        while update_receiver.recv().await.is_some() {
            info!(".");
        }
    }
}
