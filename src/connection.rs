//! # Connection Module
//! This module provides the [Connector] trait and its default implementation [`ConnectorSocket`].
//!
//! # Examples
//! ## Default Implementation
//! ```rust,ignore(bloop)
//! // Ignored because you need a running server to test this
//! use std::net::{IpAddr, SocketAddr};
//!
//! // Usage example
//! #[tokio::main]
//! async fn main() {
//!     let ip_addr: IpAddr = "127.0.0.1".parse().unwrap();
//!     let gateway_address = SocketAddr::new(ip_addr, 0);
//!     // Implicitly uses the default ConnectorSocket implementation
//!     let client = Client<ProtocolResponse, ProtocolRequest>::connect(...);
//!     let port = client.bind_socket(gateway_address).await.unwrap();
//! }
//! ```
//!
//! ## Custom Implementation
//! This example shows how to implement a custom connector using a TCP socket.
//! ```rust,ignore
//! use doip::connection::Connector;
//! use tokio::net::{TcpSocket, tcp::{OwnedReadHalf, OwnedWriteHalf}};
//! use std::net::{IpAddr, SocketAddr};
//! use std::time::Duration;
//!
//! pub struct MyConnector;
//!
//! #[async_trait::async_trait]
//! impl Connector for MyConnector {
//!    async fn establish_connection(gateway_address: SocketAddr) -> Result<(OwnedReadHalf, OwnedWriteHalf), doip::Error> {
//!        // Implement the connection logic here
//!       // For example, create a TcpSocket and connect to the address
//!        let tcp_socket = TcpSocket::new_v4().unwrap();
//!        tcp_socket.set_reuseaddr(true)?;
//!        tcp_socket.set_recv_buffer_size(1024 * 64)?;
//!        tcp_socket.set_send_buffer_size(1024 * 64)?;
//!        tcp_socket.set_nodelay(false)?;
//!        let tcp_stream = tokio::time::timeout(Duration::from_millis(5100), tcp_socket.connect(gateway_address)).await.unwrap().unwrap();
//!        Ok(tcp_stream.into_split())
//!     }
//! }
//!
//! // Usage example
//! #[tokio::main]
//! async fn main() {
//!     let ip_addr: IpAddr = "127.0.0.1".parse().unwrap();
//!     let gateway_address = SocketAddr::new(ip_addr, 0);
//!     //let client = Client<ProtocolResponse, ProtocolRequest, MyConnector>::connect(...);
//!     //let port = client.bind_socket(gateway_address).await.unwrap();
//!
//!}
//! ```
use std::{fmt::Debug, net::SocketAddr, time::Duration};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpSocket,
};
use tracing::{debug, error, info, trace};

#[async_trait::async_trait]
/// Connector trait for establishing a connection to the `DoIP` node
pub trait Connector {
    /// Establish a connection to the `DoIP` node
    async fn establish_connection(
        gateway_address: SocketAddr,
    ) -> Result<(OwnedReadHalf, OwnedWriteHalf), crate::Error>;
}

// TODO: Move this to a config file
/// Buffer size for the TCP socket
const BUFFER_SIZE: u32 = 1024 * 64;

/// ISO 13400-2:2012 Connection to the gateway node via port 13400
///
/// This socket is used to connect to the server directly.
#[derive(Clone, Copy, Debug)]
pub struct ConnectorSocket;

#[async_trait::async_trait]
impl Connector for ConnectorSocket {
    async fn establish_connection(
        gateway_address: SocketAddr,
    ) -> Result<(OwnedReadHalf, OwnedWriteHalf), crate::Error> {
        // Ensure port is TCP_PORT
        if gateway_address.port() != crate::TCP_PORT {
            return Err(crate::Error::InvalidPort(gateway_address.port()));
        }
        let tcp_socket = match gateway_address {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.set_reuseaddr(true)?;
        tcp_socket.set_recv_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_send_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_nodelay(false)?;

        match tokio::time::timeout(
            Duration::from_millis(5100),
            tcp_socket.connect(gateway_address),
        )
        .await
        {
            Ok(Ok(tcp_stream)) => {
                trace!("Connected to socket");
                Ok(tcp_stream.into_split())
            }
            Ok(Err(e)) => {
                error!("Failed to connect to socket({}): {}", gateway_address, e);
                Err(crate::Error::ConnectionClosed)
            }
            Err(e) => {
                error!("Connection timed out");
                Err(crate::Error::ConnectionTimeout(e))
            }
        }
    }
}

/// Establishes a connection via a TCP listener socket.
///
/// Instead of connecting to a remote server, this connector binds to the given
/// address and waits for an incoming connection. This is used for DoIPInt (VCC)
/// where the sensor initiates the TCP connection to the tester.
#[derive(Clone, Debug)]
pub struct ListenerSocket;

#[async_trait::async_trait]
impl Connector for ListenerSocket {
    async fn establish_connection(
        gateway_address: SocketAddr,
    ) -> Result<(OwnedReadHalf, OwnedWriteHalf), crate::Error> {
        let tcp_socket = match gateway_address {
            SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.set_reuseaddr(true)?;
        tcp_socket.set_recv_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_send_buffer_size(BUFFER_SIZE)?;
        tcp_socket.set_nodelay(false)?;

        tcp_socket.bind(gateway_address).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrNotAvailable {
                error!(
                    concat!(
                        "DoIPInt requires a server address (e.g. 198.19.16.1:13400) or an unspecified address (0.0.0.0:13400).\n",
                        "The gateway address \"{}\" is either not a valid TCP listener address,\n",
                        "or the interface is not available on this system."
                    ),
                    gateway_address
                );
            }
            crate::Error::NetworkError(e)
        })?;

        let tcp_listener = tcp_socket.listen(1)?;
        let local_addr = tcp_listener.local_addr()?;
        debug!("DoIPInt entity listening on {local_addr}");
        let result = tokio::time::timeout(Duration::from_secs(120), tcp_listener.accept()).await;

        // Drop the listener once connected so other processes can bind to the same port
        drop(tcp_listener);

        match result {
            Ok(Ok((tcp_stream, socket))) => {
                info!("CONNECTION ACCEPTED: {socket}");
                Ok(tcp_stream.into_split())
            }
            Ok(Err(e)) => {
                error!("Failed to accept connection: {e}");
                Err(crate::Error::ConnectionClosed)
            }
            Err(e) => {
                error!("Timeout: Failed to accept connection: {e}");
                Err(crate::Error::ConnectionClosed)
            }
        }
    }
}
