use std::{net::SocketAddr, time::Duration};

use futures::{SinkExt, Stream, StreamExt};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpSocket,
    },
    select,
};
use tokio_util::codec::{FramedRead, FramedWrite};

use tracing::debug;
use uds_protocol::WireFormat;

use crate::messages::*;
use crate::{client::ClientOptions, message_codec::MessageCodec, Error};

/// Buffer size for the TCP socket
const BUFFER_SIZE: u32 = 1024 * 64;

/// Inner client responsible for the handling of the connection details,
/// including creating 2 channels for sending and receiving messages.
/// it manages its inner state asynchronously, only propagating the
/// results to the outer client when the message is ready.
///
/// The inner state enters an asynchronous loop which runs in the background
///
pub(super) struct Inner<ReadDefinitions, WriteDefinitions> {
    client_options: ClientOptions,
    read_stream: FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
    write_sink: FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
}

impl<ReadDefinitions, WriteDefinitions> Inner<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat + std::fmt::Debug + 'static + std::marker::Send,
    WriteDefinitions: WireFormat + std::fmt::Debug + 'static + std::marker::Send,
{
    /// Create a new inner client.
    /// TODO: Does this need to be a Result? A constructor?
    pub async fn new(
        client_options: ClientOptions,
    ) -> Result<
        (
            FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
            FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
        ),
        Error,
    > {
        if client_options.client_logical_address < 0x0E00
            || client_options.client_logical_address > 0x0FFF
        {
            // How does the inner client deal with errors? Should they bubble up to the outer client?

            return Err(Error::InvalidClientLogicalAddress(
                client_options.client_logical_address,
            ));
        }
        let tcp_socket = match client_options.server_address {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };

        tcp_socket.set_reuseaddr(true)?;
        tcp_socket.set_recv_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_send_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_nodelay(false)?;
        let tcp_stream = tokio::time::timeout(
            Duration::from_millis(5100),
            tcp_socket.connect(client_options.server_address),
        )
        .await
        .unwrap()
        .unwrap();
        let (rx, tx) = tcp_stream.into_split();
        let read_stream = FramedRead::new(rx, MessageCodec::new());
        let write_sink = FramedWrite::new(tx, MessageCodec::new());
        Ok((read_stream, write_sink))
    }

    /// Connect to the server.
    pub async fn connect(&mut self) -> Result<(), Error> {
        todo!();
    }

    /// Reconnect to the server in case the connection is lost.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        todo!();
    }

    /// Close the connection.
    pub async fn close(&mut self) -> Result<(), Error> {
        todo!();
    }

    /// DOIP Routing Activation Request
    async fn request_routing_activation(
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

    /// UDS Tester Present AKA Alive check is an inner client only function.
    /// It is used to check if the server is alive and responding without bothering the user with the details
    async fn request_alive_check(&mut self) -> Result<AliveCheckResponse, Error> {
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

    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP inner client loop");
            loop {
                let Self {
                    read_stream,
                    write_sink,
                    ..
                } = &mut self;

                // Read a message from the read stream
                select! {
                    Some(message) = read_stream.next() => {
                        match message {
                            Ok(msg) => {
                                debug!("Received message: {:?}", msg);
                            }
                            Err(e) => {
                                debug!("Error reading message: {:?}", e);
                            }
                        }
                    }
                }
            }
        });
    }
}
