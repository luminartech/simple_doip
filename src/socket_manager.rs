//! The SocketManager is responsible for managing the socket connection and
//! handling the messages sent and received over the socket.
//!
//! It is responsible for binding the socket, sending and receiving messages,
//! and shutting down the socket when it is no longer needed.
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
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
    client::ClientOptions,
    logical_address::LogicalAddress,
    message_codec::MessageCodec,
    messages::{Message, MessageError},
    Error, TCP_PORT, TCP_TIMEOUT_GENERAL_INACTIVITY,
};

// TODO: Move this to a config file
/// Buffer size for the TCP socket
const BUFFER_SIZE: u32 = 1024 * 64;

pub trait Connector {
    /// Establish a connection to the DoIP node
    fn establish_connection(
        &self,
    ) -> impl std::future::Future<Output = Result<(OwnedReadHalf, OwnedWriteHalf), Error>> + Send;
}

/// ISO 13400-2:2012 Connecting socket
///
/// This socket is used to connect to the server directly
#[derive(Debug, Clone, Copy)]
pub struct ConnectorSocket {
    /// The address of the server to connect to
    pub addr: SocketAddr,
}
impl Connector for ConnectorSocket {
    async fn establish_connection(&self) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> {
        let tcp_socket = match self.addr {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.set_reuseaddr(true)?;
        tcp_socket.set_recv_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_send_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_nodelay(false)?;
        let tcp_stream =
            tokio::time::timeout(Duration::from_millis(5100), tcp_socket.connect(self.addr))
                .await
                .unwrap()
                .unwrap();

        Ok(tcp_stream.into_split())
    }
}

pub fn connect_to_gateway(addr: IpAddr) -> impl Connector {
    let connector = ConnectorSocket {
        addr: SocketAddr::new(addr, TCP_PORT),
    };
    connector
}

/// 1-to-1 mapping of the socket manager to the client (currently)
/// There is only one socket manager per client.
#[derive(Debug)]
pub struct SocketManager<ReadDefinitions, WriteDefinitions> {
    /// Receiver used to receive messages from the socket
    /// This is the channel that the socket manager uses to send messages back up to the client
    receiver: mpsc::Receiver<Result<Message<ReadDefinitions>, MessageError>>,
    /// Sender used to send messages to the socket
    sender: mpsc::Sender<Message<WriteDefinitions>>,
    local_port: u16,
    session_id: u16,
    /// The source address of the client connected to the socket
    source_address: Option<LogicalAddress>,
}

impl<ReadDefinitions, WriteDefinitions> SocketManager<ReadDefinitions, WriteDefinitions>
where
    ReadDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync,
    WriteDefinitions: WireFormat + std::fmt::Debug + 'static + Send + Sync,
{
    /// Creates a new SocketManager instance
    ///
    /// Binds a UDP socket for discovery
    pub async fn bind_discovery(_interface: Ipv4Addr) -> Result<Self, Error> {
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

    pub async fn with_stream(
        client_options: ClientOptions,
        rx: OwnedReadHalf,
        tx: OwnedWriteHalf,
    ) -> Result<Self, Error> {
        if !client_options
            .client_logical_address
            .is_valid_client_address()
        {
            return Err(Error::InvalidClientLogicalAddress(
                client_options.client_logical_address,
            ));
        }
        let socket_read_stream = FramedRead::new(rx, MessageCodec::new());
        let socket_write_sink = FramedWrite::new(tx, MessageCodec::new());

        let (rx_tx, rx_rx) = mpsc::channel(16);
        let (tx_tx, tx_rx) = mpsc::channel(16);

        Self::spawn_socket_loop(rx_tx, tx_rx, socket_read_stream, socket_write_sink);

        Ok(Self {
            source_address: Some(client_options.client_logical_address),
            receiver: rx_rx,
            sender: tx_tx,
            local_port: TCP_PORT, // TODO: Double check this
            session_id: 0,
        })
    }

    /// Creates a new SocketManager instance
    ///
    /// TCP socket is bound to the specified address
    pub async fn bind(client_options: ClientOptions) -> Result<Self, Error> {
        trace!("Binding socket");
        if !client_options
            .client_logical_address
            .is_valid_client_address()
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
        let (rx, tx) = if client_options.client_logical_address == LogicalAddress(0x0E00) {
            trace!("Binding to socket");
            tcp_socket.bind(SocketAddr::from((client_options.client_address, TCP_PORT)))?;

            trace!("Opening a TcpListener to socket");
            let tcp_listener = tcp_socket.listen(32)?;
            let local_addr = tcp_listener.local_addr()?;
            info!("entity listening on {}", local_addr);
            match tokio::time::timeout(
                Duration::from_secs(600), // 60 second timeout for accept
                tcp_listener.accept(),
            )
            .await?
            {
                Ok((tcp_stream, _socket)) => {
                    trace!("Accepted connection from socket");
                    tcp_stream.into_split()
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    return Err(Error::ConnectionClosed);
                }
            }
        } else {
            let tcp_stream = tokio::time::timeout(
                Duration::from_millis(5100),
                tcp_socket.connect(client_options.server_address),
            )
            .await
            .unwrap()
            .unwrap();
            tcp_stream.into_split()
        };
        let socket_read_stream = FramedRead::new(rx, MessageCodec::new());
        let socket_write_sink = FramedWrite::new(tx, MessageCodec::new());

        let (rx_tx, rx_rx) = mpsc::channel(16);
        let (tx_tx, tx_rx) = mpsc::channel(16);

        Self::spawn_socket_loop(rx_tx, tx_rx, socket_read_stream, socket_write_sink);

        Ok(Self {
            source_address: Some(client_options.client_logical_address),
            receiver: rx_rx,
            sender: tx_tx,
            local_port: TCP_PORT, // TODO: Double check this
            session_id: 0,
        })
    }

    /// Send a message to the target address
    pub async fn send(&mut self, message: Message<WriteDefinitions>) -> Result<(), Error> {
        self.sender.send(message).await.map_err(|e| {
            error!("Failed to send message: {}", e);
            Error::ConnectionClosed
        })?;
        self.session_id += 1;
        Ok(())
    }

    /// Receive a message from the receiver/Request channel
    pub async fn receive(&mut self) -> Option<Result<Message<ReadDefinitions>, MessageError>> {
        self.receiver.recv().await
    }

    /// Receive a message with a timeout
    pub async fn receive_timeout(
        &mut self,
        timeout: Duration,
    ) -> Option<Result<Message<ReadDefinitions>, MessageError>> {
        tokio::time::timeout(timeout, self.receiver.recv())
            .await
            .unwrap()
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
        rx_tx: mpsc::Sender<Result<Message<ReadDefinitions>, MessageError>>,
        mut tx_rx: mpsc::Receiver<Message<WriteDefinitions>>,
        mut socket_read_stream: FramedRead<OwnedReadHalf, MessageCodec<ReadDefinitions>>,
        mut socket_write_sink: FramedWrite<OwnedWriteHalf, MessageCodec<WriteDefinitions>>,
    ) {
        tokio::spawn(async move {
            // General TCP activity timeout
            // this is used to close the socket if there is no activity for a while
            let mut last_activity = tokio::time::Instant::now();

            loop {
                select! {
                    _ = tokio::time::sleep_until(last_activity + TCP_TIMEOUT_GENERAL_INACTIVITY) => {
                        info!("General inactivity timeout reached, closing socket");
                        // TODO: Do we need to send an update message to update the connection state?
                        // or should the connection state be located in the socket manager?
                        break;
                    }
                    // Once there is information in the Response/Read stream we'll do work on it
                    // and send it along to the receiver on the other end
                    result = socket_read_stream.next() => {
                        match result {
                            Some(Err(e)) => {
                                error!("Error decoding message: {:?}", e)
                            }
                            Some(message) => {
                                // Update the last activity time
                                last_activity = tokio::time::Instant::now();
                                trace!("Received response from socket: {:?}", message);
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
                                // Update the last activity time
                                last_activity = tokio::time::Instant::now();

                                trace!("Sending request to socket: {:?}", message);
                                match socket_write_sink.send(&message).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        error!("Error sending message to socket: {:?}", e);
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
                    }
                }
            }
        });
    }
}
