use doip::{
    client::{Client, ClientOptions},
    connection::Connector,
    logical_address::LogicalAddress,
    messages::ProtocolVersion,
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
pub struct ListenerSocket;
impl Connector for ListenerSocket {
    async fn establish_connection(
        gateway_address: SocketAddr,
    ) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> {
        let tcp_socket = match gateway_address {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.set_reuseaddr(true)?;
        tcp_socket.set_recv_buffer_size(1024 * 64)?;
        tcp_socket.set_send_buffer_size(1024 * 64)?;
        tcp_socket.set_nodelay(false)?;

        tcp_socket.bind(gateway_address)?;

        let tcp_listener = tcp_socket.listen(32)?;
        let local_addr = tcp_listener.local_addr()?;
        info!("entity listening on {}", local_addr);
        match tokio::time::timeout(
            Duration::from_secs(60), // 60 second timeout for accept
            tcp_listener.accept(),
        )
        .await
        {
            Ok(Ok((tcp_stream, _socket))) => {
                trace!("Accepted connection from socket");
                Ok(tcp_stream.into_split())
            }
            Ok(Err(e)) => {
                error!("Failed to accept connection: {}", e);
                Err(doip::Error::ConnectionClosed)
            }
            Err(e) => {
                error!("Timeout: Failed to accept connection: {}", e);
                Err(doip::Error::ConnectionClosed)
            }
        }
    }
}

type InternalClient = Client<ProtocolResponse, ProtocolRequest, ListenerSocket>;
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

    let mut client = InternalClient::connect(client_options).await?;
    let port = client
        .bind_socket(SocketAddr::new(client_options.client_address, 0))
        .await?;

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
