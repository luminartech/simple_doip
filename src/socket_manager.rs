use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
};

use tokio::{net::UdpSocket, select, sync::mpsc};
use tracing::{error, info, trace};
use uds_protocol::WireFormat;

use crate::{
    client_inner::ControlResponse, message_codec::MessageCodec, messages::Message, Error, TCP_PORT,
    UDP_DISCOVERY_PORT,
};

#[derive(Debug)]
pub struct SocketManager<DiagnosticDefinitions> {
    receiver: mpsc::Receiver<Result<Message<DiagnosticDefinitions>, Error>>,
    sender: mpsc::Sender<(SocketAddrV4, Message<DiagnosticDefinitions>)>,
    local_port: u16,
    session_id: u16,
}

impl<DiagnosticDefinitions> SocketManager<DiagnosticDefinitions>
where
    DiagnosticDefinitions: WireFormat + std::fmt::Debug + 'static + Send,
{
    /// Creates a new SocketManager instance
    pub async fn bind_discovery(interface: Ipv4Addr) -> Result<Self, Error> {
        let (rx_tx, rx_rx) = mpsc::channel(16);
        let (tx_tx, tx_rx) = mpsc::channel(16);
        let bind_addr =
            std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), UDP_DISCOVERY_PORT);
        let socket = UdpSocket::bind(bind_addr).await?;

        Self::spawn_socket_loop(socket, rx_tx, tx_rx);

        Ok(Self {
            receiver: rx_rx,
            sender: tx_tx,
            local_port: UDP_DISCOVERY_PORT,
            session_id: 0,
        })
    }

    /// Send a message to the target address
    pub async fn send(
        &mut self,
        target_addr: SocketAddrV4,
        message: Message<DiagnosticDefinitions>,
    ) -> Result<ControlResponse, Error> {
        if let Err(e) = self.sender.send((target_addr, message)).await {
            error!("Failed to send message: {}", e);
        }
        self.session_id += 1;
        Ok(ControlResponse::Success)
    }

    /// Receive a message from the socket
    pub async fn receive(&mut self) -> Option<Result<Message<DiagnosticDefinitions>, Error>> {
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
        drop(sender);
        trace!("receive any remaining messages");
        _ = receiver.recv().await;
    }

    fn spawn_socket_loop(
        socket: UdpSocket,
        rx_tx: mpsc::Sender<Result<Message<DiagnosticDefinitions>, Error>>,
        mut tx_rx: mpsc::Receiver<(SocketAddrV4, Message<DiagnosticDefinitions>)>,
    ) {
        tokio::spawn(async move {
            let mut buf = vec![0; 1400];
            loop {
                select! {
                    result = socket.recv_from(&mut buf) => {
                    }

                }
            }
        });
    }
}
