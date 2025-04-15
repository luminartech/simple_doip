use std::{net::SocketAddr, time::Duration};

use futures::{SinkExt, Stream, StreamExt};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpSocket,
    },
    select,
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};

use tracing::debug;
use uds_protocol::WireFormat;

use crate::{client, messages::*};
use crate::{client::ClientOptions, message_codec::MessageCodec, Error};

/// Buffer size for the TCP socket
const BUFFER_SIZE: u32 = 1024 * 64;

pub(super) enum Control<ReadDefinitions> {
    UDSMessage(Message<ReadDefinitions>),
    RoutingActivation,
}

/// Results of a Control message
#[derive(Debug)]
pub enum ControlResponse {
    Success,
    SocketBind(u16),
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
    control_receiver: mpsc::Receiver<FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>>,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request: Option<Control<ReadDefinitions>>,
}

impl<ReadDefinitions, WriteDefinitions> Inner<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat + std::fmt::Debug + 'static + Send,
    WriteDefinitions: WireFormat + std::fmt::Debug + 'static + Send,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub async fn spawn(
        client_options: ClientOptions,
    ) -> (
        mpsc::Sender<FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>>,
        mpsc::Receiver<FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>>,
    ) {
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        let inner = Inner {
            client_options,
            active_request: None,
            control_receiver,
            update_sender,
        };
        inner.run();
        (control_sender, update_receiver)
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

    async fn handle_control_message(&mut self) {
        todo!()
    }

    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP inner client loop");
            loop {
                let Self {
                    control_receiver,
                    update_sender,
                    ..
                } = &mut self;

                // Read a message from the read stream
                select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(125)) => {}
                    // Receive a control message
                    ctrl = control_receiver.recv() => {
                        if let Some(ctrl) = ctrl {
                            assert!(self.active_request.is_none());
                            // debug!("Received control message: {:?}", ctrl);
                            self.active_request = Some(ctrl);
                        } else {
                            // The sender has been dropped, so we should exit
                            break;
                        }
                    }
                }
                self.handle_control_message().await;
            }
        });
    }
}
