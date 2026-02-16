//! The `SocketManager` is responsible for managing the socket connection and
//! handling the messages sent and received over the socket.
//!
//! It is responsible for binding the socket, sending and receiving messages,
//! and shutting down the socket when it is no longer needed.
use crate::{
    Error, TCP_TIMEOUT_GENERAL_INACTIVITY,
    client::ClientOptions,
    connection,
    message_codec::MessageCodec,
    messages::{Message, MessageError},
};
use futures::{SinkExt, StreamExt};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    select,
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{debug, error, info, trace};

/// 1-to-1 mapping of the socket manager to the client (currently)
/// There is only one socket manager per client.
#[derive(Debug)]
pub struct SocketManager<Conn> {
    /// Receiver used to receive messages from the socket
    /// This is the channel that the socket manager uses to send messages back up to the client
    receiver: mpsc::Receiver<Result<Message, MessageError>>,
    /// Sender used to send messages to the socket
    sender: mpsc::Sender<Message>,
    local_port: u16,
    session_id: u16,

    _phantom: std::marker::PhantomData<Conn>,
}

impl<Conn> SocketManager<Conn>
where
    Conn: connection::Connector + 'static + Send + Sync,
{
    /// Creates a new `SocketManager` instance
    ///
    /// TCP socket is bound to the specified address
    ///
    /// # Errors
    /// Returns an [`Error`] if the client logical address is invalid or the
    /// TCP connection cannot be established
    pub async fn bind(
        client_options: ClientOptions,
        gateway_address: SocketAddr,
    ) -> Result<Self, Error> {
        if !client_options
            .client_logical_address
            .is_valid_client_address()
        {
            return Err(Error::InvalidClientLogicalAddress(
                client_options.client_logical_address,
            ));
        }

        // Call the connection - this might be overridden by the user
        let (rx, tx) = match Conn::establish_connection(gateway_address).await {
            Ok((rx, tx)) => (rx, tx),
            Err(e) => {
                error!("Failed to establish connection: {e} on {gateway_address}");
                return Err(e);
            }
        };

        let socket_read_stream = FramedRead::new(rx, MessageCodec::new());
        let socket_write_sink = FramedWrite::new(tx, MessageCodec::new());

        let (rx_tx, rx_rx) = mpsc::channel(16);
        let (tx_tx, tx_rx) = mpsc::channel(16);

        Self::spawn_socket_loop(rx_tx, tx_rx, socket_read_stream, socket_write_sink);

        Ok(Self {
            receiver: rx_rx,
            sender: tx_tx,
            local_port: gateway_address.port(),
            session_id: 0,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Send a message to the target address
    ///
    /// # Errors
    /// Returns an [`Error::ConnectionClosed`] if the message cannot be sent
    pub async fn send(&mut self, message: Message) -> Result<(), Error> {
        self.sender.send(message).await.map_err(|e| {
            error!("Failed to send message: {}", e);
            Error::ConnectionClosed
        })?;
        self.session_id += 1;
        Ok(())
    }

    /// Receive a message from the receiver/Request channel
    pub async fn receive(&mut self) -> Option<Result<Message, MessageError>> {
        self.receiver.recv().await
    }

    /// Receive a message from the socket, returning `None` if the timeout elapses
    ///
    /// # Panics
    /// Panics if the internal receiver channel is closed
    pub async fn receive_timeout(
        &mut self,
        timeout: Duration,
    ) -> Option<Result<Message, MessageError>> {
        tokio::time::timeout(timeout, self.receiver.recv())
            .await
            .unwrap()
    }

    /// Return the current session ID
    #[must_use]
    pub fn session_id(&self) -> u16 {
        self.session_id
    }

    /// Return the local TCP port this socket is bound to
    #[must_use]
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
        rx_tx: mpsc::Sender<Result<Message, MessageError>>,
        mut tx_rx: mpsc::Receiver<Message>,
        mut socket_read_stream: FramedRead<OwnedReadHalf, MessageCodec>,
        mut socket_write_sink: FramedWrite<OwnedWriteHalf, MessageCodec>,
    ) {
        tokio::spawn(async move {
            // General TCP activity timeout
            // this is used to close the socket if there is no activity for a while
            let mut last_activity = tokio::time::Instant::now();

            loop {
                select! {
                    () = tokio::time::sleep_until(last_activity + TCP_TIMEOUT_GENERAL_INACTIVITY) => {
                        info!("General inactivity timeout reached, closing socket");
                        // TODO: Do we need to send an update message to update the connection state?
                        // or should the connection state be located in the socket manager?
                        break;
                    }
                    // Once there is information in the Response/Read stream we'll do work on it
                    // and send it along to the receiver on the other end
                    //
                    result = socket_read_stream.next() => {
                        match result {
                            // Decoding the message can fail, so we handle that here
                            Some(Err(e)) => {
                                last_activity = tokio::time::Instant::now();
                                if let MessageError::Io(ref io_err) = e {
                                    if io_err.kind() == std::io::ErrorKind::ConnectionReset {
                                        info!(concat!("Connection reset by peer, closing socket\n", "{:?}"), io_err);
                                        // The socket has been closed by the remote end, so we should exit
                                        break;
                                    }
                                    error!(concat!("{:?}\n",
                                        "Check that you are not sending too many requests to the server.",
                                        "The server may be closing the connection due to overload."
                                    ), io_err);
                                    // The socket has been closed by the remote end, so we should exit
                                    break;
                                }
                                error!("Error decoding message: {:?}", e.to_string());

                                // send a MessageError to the receiver
                                let _ = rx_tx.send(Err(e)).await;
                            }
                            Some(message) => {
                                // Update the last activity time
                                last_activity = tokio::time::Instant::now();
                                trace!("A: STREAM INCOMING: {:?}", message);
                                if rx_tx.send(message).await.is_err() {
                                    info!("Socket Dropping");
                                    // The receiver has been dropped, so we should exit
                                    break;
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
                        let Some(message) = message else {
                            debug!("Socket Dropping");
                            // The sender has been dropped, so we should exit
                            break;
                        };
                        // Update the last activity time
                        last_activity = tokio::time::Instant::now();

                        trace!("OUTGOING: {:?}", message);
                        if let Err(e) = socket_write_sink.send(&message).await {
                            error!("Error sending message to socket: {:?}", e);
                            break;
                        }
                    }
                }
            }
        });
    }
}
