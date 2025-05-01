use crate::{client_inner::ControlMessage, messages::RoutingActivationRequest};
use crate::{
    client_inner::Inner,
    messages::{
        ActivationTypeCode, AliveCheckResponse, Header, Message, MessageError, Payload,
        PayloadType, ProtocolVersion, RoutingActivationResponse,
    },
    traits::WirePayload,
    Error, LogicalAddress,
};

use std::net::{IpAddr, SocketAddr};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, trace};

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressType {
    Logical,
    Physical,
}

/// Follows the Facade pattern, providing a simplified interface to the client
#[derive(Debug)]
pub struct Client<ReadDefinitions, WriteDefinitions> {
    pub client_options: ClientOptions,
    /// Sends messages from the user to the inner client
    control_sender: mpsc::Sender<ControlMessage<ReadDefinitions, WriteDefinitions>>,
    /// Receives messages from the inner client to the user
    update_receiver: mpsc::Receiver<Result<Message<ReadDefinitions>, MessageError>>,
}

impl<
        ReadDefinitions: WirePayload + 'static + Sync + Send + Clone,
        WriteDefinitions: WirePayload + 'static + Sync + Send + Clone,
    > Client<ReadDefinitions, WriteDefinitions>
{
    /// Create a DoIP connection.
    /// The target port defaults to [`crate::TCP_PORT`].
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        let (control_sender, update_receiver) = Inner::spawn(client_options);
        Ok(Self {
            client_options,
            control_sender,
            update_receiver,
        })
    }

    pub async fn bind_socket(&mut self) -> Result<u16, Error> {
        let (response, message) = ControlMessage::create_bind_socket_message();
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

    pub async fn diagnostic_message(
        &mut self,
        address_type: AddressType,
        user_data: WriteDefinitions,
    ) -> Result<(), Error> {
        let message = Message::<WriteDefinitions>::diagnostic_message(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            match address_type {
                AddressType::Logical => self.client_options.server_logical_address,
                AddressType::Physical => self.client_options.server_physical_address,
            },
            user_data,
        );
        // Send the message
        let response = self.send_message(&message).await;
        match response {
            Ok(response) => {
                // Send the response to the update channel
                Ok(())
            }
            Err(err) => {
                // Send the error to the update channel
                Ok(())
                // self.update_receiver.send(Err(err)).await.unwrap();
            }
        }
    }

    /// Send a request to the server and wait for a response (which is returned)
    async fn send_message(
        &mut self,
        control: &Message<WriteDefinitions>,
    ) -> Result<Message<ReadDefinitions>, Error> {
        // Create new request and response channels to await the response of
        let (response, message) = ControlMessage::send_request(control);
        self.control_sender.send(message).await.unwrap();
        response.await.unwrap()
    }

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
