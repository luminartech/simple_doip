use crate::{
    client,
    client_inner::Inner,
    message_codec::MessageCodec,
    messages::{
        ActivationTypeCode, AliveCheckResponse, Header, Message, MessageError, Payload,
        PayloadType, ProtocolVersion, RoutingActivationResponse,
    },
    traits::WirePayload,
    Error,
};

use futures::{SinkExt, StreamExt};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpSocket,
};
use tokio_util::codec::{FramedRead, FramedWrite};

/// DoIP client options used to specify connection info
/// Derive `Serialize` and `Deserialize` for use in config files
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientOptions {
    /// Server IP address and port
    pub server_address: SocketAddr,
    /// Target logical addresses, uniquely identifies the ECU to be diagnosed.
    /// Valid range: 0x0001 - 0x0DFF
    pub server_logical_address: u16,
    /// Valid range: 0x0001 - 0x0DFF
    pub server_physical_address: u16,
    /// Local ip address to bind the TCP and UDP sockets to, e.g. `0.0.0.0`. The port is randomly chosen.
    pub client_address: IpAddr,
    /// Valid range: 0x0E00 - 0x0FFF
    pub client_logical_address: u16,
    /// Which protocol version the client should
    pub protocol_version: ProtocolVersion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressType {
    Logical,
    Physical,
}

/// Follows the Facade pattern, providing a simplified interface to the client
pub struct Client<ReadDefinitions, WriteDefinitions> {
    pub client_options: ClientOptions,
    read_stream: FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
    write_sink: FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
}

impl<ReadDefinitions: WirePayload + 'static, WriteDefinitions: WirePayload + 'static>
    Client<ReadDefinitions, WriteDefinitions>
{
    /// Create a DoIP connection.
    /// The target port defaults to [`SERVER_TCP_PORT`].
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        let (read_stream, write_sink) =
            Inner::<ReadDefinitions, WriteDefinitions>::new(client_options).await?;

        Ok(Self {
            client_options,
            read_stream,
            write_sink,
        })
    }

    pub async fn close(&mut self) -> Result<(), Error> {
        self.write_sink.flush().await?;
        self.write_sink.close().await?;
        Ok(())
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
        self.write_sink.send(&message).await?;
        Ok(())
    }
}
