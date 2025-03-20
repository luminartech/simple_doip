use crate::{
    message_codec::MessageCodec,
    messages::{
        ActivationTypeCode, AliveCheckResponse, Header, Message, MessageError, Payload,
        PayloadType, ProtocolVersion, RoutingActivationResponse,
    },
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
use uds_protocol::SingleValueWireFormat;

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

pub struct Client<ReadDefinitions, WriteDefinitions> {
    pub client_options: ClientOptions,
    read_stream: FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
    write_sink: FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
}

impl<ReadDefinitions: SingleValueWireFormat, WriteDefinitions: SingleValueWireFormat>
    Client<ReadDefinitions, WriteDefinitions>
{
    /// Create a DoIP connection.
    /// The target port defaults to [`SERVER_TCP_PORT`].
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        if client_options.client_logical_address < 0x0E00
            || client_options.client_logical_address > 0x0FFF
        {
            return Err(Error::InvalidClientLogicalAddress(
                client_options.client_logical_address,
            ));
        }

        let tcp_socket = match client_options.server_address {
            SocketAddr::V4(_) => TcpSocket::new_v4()?,
            SocketAddr::V6(_) => TcpSocket::new_v6()?,
        };
        tcp_socket.set_reuseaddr(true)?;
        const BUFFER_SIZE: u32 = 1024 * 64;
        tcp_socket.set_recv_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_send_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_nodelay(false)?;
        let tcp_stream = tokio::time::timeout(
            Duration::from_millis(500),
            tcp_socket.connect(client_options.server_address),
        )
        .await??;
        let (rx, tx) = tcp_stream.into_split();
        let read_stream = FramedRead::new(rx, MessageCodec::<ReadDefinitions>::new());
        let write_sink = FramedWrite::new(tx, MessageCodec::<WriteDefinitions>::new());

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

    pub async fn request_routing_activation(
        &mut self,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Result<RoutingActivationResponse, Error> {
        let message = Message::<WriteDefinitions>::routing_activation_request(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            activation_type,
            reserved_vehicle_manufacturer,
        );

        self.write_sink.send(&message).await?;
        match self.read_tcp_message().await {
            Some(Ok(response_message)) => {
                if let Payload::RoutingActivationResponse(response) = response_message.payload {
                    Ok(response)
                } else {
                    Err(Error::UnexpectedMessageType(
                        response_message.header.payload_type,
                    ))
                }
            }
            Some(Err(error)) => Err(error),
            None => Err(Error::ConnectionClosed),
        }
    }

    pub async fn request_alive_check(&mut self) -> Result<AliveCheckResponse, Error> {
        let header = Header::new(
            self.client_options.protocol_version,
            PayloadType::AliveCheckRequest,
            0,
        );
        let payload = Payload::<WriteDefinitions>::AliveCheckRequest;
        let message = Message { header, payload };
        self.write_sink.send(&message).await?;

        let response_message = self.read_tcp_message().await.unwrap()?;
        if let Payload::AliveCheckResponse(response_payload) = response_message.payload {
            Ok(response_payload)
        } else {
            Err(Error::UnexpectedMessageType(
                response_message.header.payload_type,
            ))
        }
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

    pub async fn read_tcp_message(&mut self) -> Option<Result<Message<ReadDefinitions>, Error>> {
        // Unwrap here is to unwrap the option, not the result
        match self.read_stream.next().await {
            None => None,
            Some(result) => match result {
                Ok(message) => Some(Ok(message)),
                Err(error) => {
                    if let MessageError::UdsProtocol(uds_protocol::Error::IoError(err)) = &error {
                        if err.kind() == std::io::ErrorKind::UnexpectedEof {
                            return None;
                        }
                    }

                    println!("Error reading message: {:?}", error);
                    Some(Err(Error::from(error)))
                }
            },
        }
    }
}
