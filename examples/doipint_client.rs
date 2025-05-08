use doip::{
    client::{Client, ClientOptions},
    logical_address::LogicalAddress,
    messages::ProtocolVersion,
    socket_manager::Connector,
    Error, TCP_PORT,
};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpSocket,
};
use tracing::{error, info, trace};
use tracing_subscriber;
use uds_protocol::{ProtocolRequest, ProtocolResponse};

/// Sets up a TCP listener on a client address and waits for a connection from the server
pub struct ListenerSocket {
    /// The client opening the listener socket
    pub client_address: SocketAddr,
}
impl Connector for ListenerSocket {
    async fn establish_connection(&self) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> {
        let tcp_socket = match self.client_address {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.bind(self.client_address)?;
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
                Ok(tcp_stream.into_split())
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
                Err(Error::ConnectionClosed)
            }
        }
    }
}

/// This is a simple client that creates a TCP socket and waits for a connection from the server
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_max_level(tracing::Level::TRACE)
        .init();

    info!("Starting DOIP Listener client");
    let server_ip: IpAddr = "198.19.12.54".parse()?;

    let client_options: ClientOptions = ClientOptions {
        server_address: SocketAddr::from((server_ip, TCP_PORT)),
        server_logical_address: LogicalAddress(0x1FFF),
        server_physical_address: LogicalAddress(0x1D35),
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: LogicalAddress(0x0E80),
        protocol_version: ProtocolVersion::V2012,
        routing_activation_options: None,
    };

    let mut client = Client::<ProtocolResponse, ProtocolRequest>::connect(client_options).await?;
    let connector = ListenerSocket {
        client_address: SocketAddr::new(client_options.client_address, 0),
    };
    let (rx, tx) = connector.establish_connection().await?;
    let port = client.bind_socket(rx, tx).await?;

    info!("Bound to port: {}", port);

    let resp = client
        .send_diagnostic_message(
            doip::client::AddressType::Physical,
            ProtocolRequest::diagnostic_session_control(
                true,
                uds_protocol::DiagnosticSessionType::ProgrammingSession,
            ),
        )
        .await?;
    info!("Sent diagnostic message and received response {:#?}", resp);
    client.shut_down().await;
    Ok(())
}
