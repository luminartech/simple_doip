//! # Connection Module
//!
//! Provides the [`Connector`] trait and its default implementation [`ConnectorSocket`].
//!
//! [`Connector`] is the extension point for callers who need to control how the TCP
//! connection is established — a non-standard port, custom socket options, or a
//! pre-existing stream. [`ConnectorSocket`] connects to [`crate::TCP_PORT`] and refuses
//! any other port, so tests and non-standard deployments substitute their own.
//!
//! # Examples
//!
//! ## Default implementation
//!
//! ```no_run
//! use simple_doip::client::{Client, ClientOptions, RoutingActivationOptions};
//! use simple_doip::connection::ConnectorSocket;
//! use simple_doip::messages::{ActivationTypeCode, ProtocolVersion};
//! use simple_doip::LogicalAddress;
//! use std::net::{IpAddr, SocketAddr};
//!
//! # async fn example() -> Result<(), simple_doip::Error> {
//! let options = ClientOptions {
//!     server_address: SocketAddr::new("127.0.0.1".parse().unwrap(), simple_doip::TCP_PORT),
//!     server_logical_address: LogicalAddress(0x0001),
//!     server_physical_address: LogicalAddress(0x0001),
//!     client_address: IpAddr::from([0, 0, 0, 0]),
//!     client_logical_address: LogicalAddress(0x0E01),
//!     protocol_version: ProtocolVersion::V2012,
//!     routing_activation_options: Some(RoutingActivationOptions {
//!         activation_type: ActivationTypeCode::Default,
//!         oem_specific: None,
//!     }),
//! };
//! // The turbofish is required even though `Client<Conn>` defaults to `ConnectorSocket`:
//! // a type parameter's default is not used for inference on an associated-function call,
//! // so omitting it fails with E0283 "type annotations needed".
//! let client = Client::<ConnectorSocket>::connect(options).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom implementation
//!
//! ```no_run
//! use simple_doip::connection::Connector;
//! use std::net::SocketAddr;
//! use std::time::Duration;
//! use tokio::net::{TcpSocket, tcp::{OwnedReadHalf, OwnedWriteHalf}};
//!
//! pub struct MyConnector;
//!
//! #[async_trait::async_trait]
//! impl Connector for MyConnector {
//!     async fn establish_connection(
//!         gateway_address: SocketAddr,
//!     ) -> Result<(OwnedReadHalf, OwnedWriteHalf), simple_doip::Error> {
//!         // IPv4 only, for brevity. `ConnectorSocket` below picks `new_v4`/`new_v6`
//!         // from `gateway_address`; do the same if you need to support both.
//!         let tcp_socket = TcpSocket::new_v4()?;
//!         tcp_socket.set_reuseaddr(true)?;
//!         tcp_socket.set_nodelay(true)?;
//!         let tcp_stream = tokio::time::timeout(
//!             Duration::from_millis(5100),
//!             tcp_socket.connect(gateway_address),
//!         )
//!         .await??;
//!         Ok(tcp_stream.into_split())
//!     }
//! }
//! ```
use std::{boxed::Box, fmt::Debug, net::SocketAddr, time::Duration};
use tokio::net::{
    TcpSocket,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};
use tracing::{error, trace};

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
        tcp_socket.set_nodelay(true)?;

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
                Err(crate::Error::NetworkError(e))
            }
            Err(e) => {
                error!("Connection timed out");
                Err(crate::Error::ConnectionTimeout(e))
            }
        }
    }
}
