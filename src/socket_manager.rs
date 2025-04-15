use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use futures::{SinkExt, StreamExt};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpSocket,
    },
    select,
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{error, info, trace};
use uds_protocol::WireFormat;

use crate::{
    client::ClientOptions, client_inner::ControlResponse, message_codec::MessageCodec,
    messages::Message, Error, TCP_PORT,
};

// TODO: Move this to a config file
/// Buffer size for the TCP socket
const BUFFER_SIZE: u32 = 1024 * 64;

#[derive(Debug)]
pub struct SocketManager<ReadDefinitions, WriteDefinitions> {
    receiver: mpsc::Receiver<Result<Message<ReadDefinitions>, Error>>,
    sender: mpsc::Sender<Message<WriteDefinitions>>,
    local_port: u16,
    session_id: u16,
}

impl<ReadDefinitions, WriteDefinitions> SocketManager<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync,
    WriteDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync,
{
    /// Creates a new SocketManager instance
    ///
    /// Binds a UDP socket for discovery
    pub async fn bind_discovery(interface: Ipv4Addr) -> Result<Self, Error> {
        unimplemented!("UDP discovery not implemented yet");
        // let (rx_tx, rx_rx) = mpsc::channel(16);
        // let (tx_tx, tx_rx) = mpsc::channel(16);
        // let bind_addr =
        //     std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), UDP_DISCOVERY_PORT);
        // let socket = UdpSocket::bind(bind_addr).await?;

        // // Self::spawn_socket_loop(socket, rx_tx, tx_rx);

        // Ok(Self {
        //     receiver: rx_rx,
        //     sender: tx_tx,
        //     local_port: bind_addr.port(),
        //     session_id: 0,
        // })
    }

    /// Creates a new SocketManager instance
    ///
    /// TCP socket is bound to the specified address
    pub async fn bind(client_options: ClientOptions) -> Result<Self, Error> {
        if client_options.client_logical_address < 0x0E00
            || client_options.client_logical_address > 0x0FFF
        {
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
        let socket_read_stream = FramedRead::new(rx, MessageCodec::new());
        let socket_write_sink = FramedWrite::new(tx, MessageCodec::new());

        let (rx_tx, rx_rx) = mpsc::channel(16);
        let (tx_tx, tx_rx) = mpsc::channel(16);

        Self::spawn_socket_loop(rx_tx, tx_rx, socket_read_stream, socket_write_sink);

        Ok(Self {
            receiver: rx_rx,
            sender: tx_tx,
            local_port: TCP_PORT, // TODO: Double check this
            session_id: 0,
        })
    }

    /// Send a message to the target address
    pub async fn send(
        &mut self,
        message: Message<WriteDefinitions>,
    ) -> Result<ControlResponse, Error> {
        if let Err(e) = self.sender.send(message).await {
            error!("Failed to send message: {}", e);
        }
        self.session_id += 1;
        Ok(ControlResponse::Success)
    }

    /// Receive a message from the receiver/Request channel
    pub async fn receive(&mut self) -> Option<Result<Message<ReadDefinitions>, Error>> {
        self.receiver.recv().await
    }

    pub fn session_id(&self) -> u16 {
        self.session_id
    }

    pub fn port(&self) -> u16 {
        self.local_port
    }

    /// Shutdown the socket manager
    /// This will close the socket and stop the event loop
    /// It will also drop the sender and receiver channels
    pub async fn shut_down(self) {
        let Self {
            sender,
            mut receiver,
            ..
        } = self;
        trace!("Shutting down socket manager - Sender");
        // First stop accepting messages before we drop
        receiver.close();
        drop(sender);
        trace!("receive any remaining messages");
        _ = receiver.recv().await;
    }

    /// Spawn the socket loop to get messages from the socket
    fn spawn_socket_loop(
        rx_tx: mpsc::Sender<Result<Message<ReadDefinitions>, Error>>,
        mut tx_rx: mpsc::Receiver<Message<WriteDefinitions>>,
        mut socket_read_stream: FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
        mut socket_write_sink: FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
    ) {
        tokio::spawn(async move {
            loop {
                select! {
                    // Once there is information in the Response/Read stream we'll do work on it
                    // and send it along to the receiver on the other end
                    result = socket_read_stream.next() => {
                        match result {
                            Some(Err(e)) => {
                                error!("Error decoding message: {:?}", e)
                            }
                            Some(message) => {
                                let message = message.map_err(|e| Error::from(e));
                                trace!("Received: {:?}", message);
                                match rx_tx.send( message ).await {
                                    Ok(_) => {}
                                    Err(_) => {
                                        info!("Socket Dropping");
                                        // The receiver has been dropped, so we should exit
                                        break;
                                    }
                                }
                            }
                            None => {
                                info!("Socket Dropping");
                                // The sender has been dropped, so we should exit
                                break;
                            }
                        }
                    },
                    // maps to self.receiver
                    message = tx_rx.recv() => {
                        match message {
                            Some(message) => {
                                trace!("Sending: {:?}", message);
                                socket_write_sink.send(&message).await.unwrap();
                            }
                            None => {
                                info!("Socket Dropping");
                                // The sender has been dropped, so we should exit
                                break;
                            }
                        }
                    }
                }
            }
        });
    }
}
