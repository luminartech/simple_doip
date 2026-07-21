//! `DoIP` entity (server) side of a connection: accepts tester TCP connections,
//! drives routing activation, and dispatches diagnostic messages to the
//! implementing application via the [`Server`] trait.

use crate::{
    Error, TCP_PORT,
    logical_address::LogicalAddress,
    message_codec::MessageCodec,
    messages::{
        DiagnosticMessage, DiagnosticPowerModeCode, FurtherActionRequired, OwnedMessage,
        OwnedPayload, ProtocolVersion, RoutingActivationRequest, VehicleIdentificationResponse,
        VinGidSyncStatus,
    },
};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::{
    boxed::Box,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{error, warn};

/// Identifies the tester on the other end of a `DoIP` TCP connection, passed to
/// [`ServerConnectionHandler`] methods so an implementation can tell which peer
/// is asking.
#[derive(Debug)]
pub struct ClientConnectionInfo {
    /// IP address of the tester's end of the TCP connection.
    pub ip_address: IpAddr,
    /// Intended to carry the logical address the tester identified itself with
    /// during routing activation.
    ///
    /// **Currently always `0x0000`.** The server does not yet track per-connection
    /// state, so the tester's routing activation source address is never
    /// propagated here and this field is hard-coded. As a consequence the default
    /// [`ServerConnectionHandler::alive_check`] implementation answers with source
    /// address `0x0000`. Do not treat this field as carrying real data.
    pub logical_address: LogicalAddress,
}

/// RAII guard that increments `active_connections` on creation and decrements it
/// exactly once when dropped, regardless of which path the connection handler
/// exits through (early return via `?`, an explicit `return`, or a panic).
struct ActiveConnectionGuard<'a> {
    active_connections: &'a AtomicUsize,
}

impl<'a> ActiveConnectionGuard<'a> {
    fn new(active_connections: &'a AtomicUsize) -> Self {
        active_connections.fetch_add(1, Ordering::Relaxed);
        Self { active_connections }
    }
}

impl Drop for ActiveConnectionGuard<'_> {
    fn drop(&mut self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}
/// Trait for handling `DoIP` connections as a server.
/// Implement this trait to create a custom `DoIP` server.
/// Most protocol functions have a simple, default implementation
#[async_trait]
pub trait ServerConnectionHandler {
    // Required Functions
    // These functions must be implemented by the server implementation

    /// Get the Vehicle Identification Number for this server
    fn get_vin(&self) -> [u8; 17];

    /// Get the ECU logical address for this server
    fn get_logical_address(&self) -> LogicalAddress;

    /// Get the unique entity ID for this server
    /// This is usually the MAC address of the network interface.
    fn get_entity_id(&self) -> [u8; 6];

    /// Get the unique group identification
    /// Optional field, return `None` if not set.
    fn get_group_id(&self) -> Option<[u8; 6]>;

    /// Decide whether to grant routing activation for the requesting tester, and
    /// build the response message to send back.
    ///
    /// # Errors
    /// Returns an [`Error`] if the routing activation response cannot be
    /// constructed.
    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error>;

    /// Handle a diagnostic message addressed to this entity
    /// and build the acknowledgement/response message to send back.
    ///
    /// # Errors
    /// Returns an [`Error`] if the message cannot be processed or the response
    /// cannot be constructed.
    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
    ) -> Result<OwnedMessage, Error>;

    // Optional Functions
    // These functions *may* be overridden to provide custom behavior
    // Default functionality is very simplistic and may not be suitable for production use

    /// Respond to an Identification request with the identity parameters provided by the trait implementer
    ///
    /// # Errors
    /// Returns an [`Error`] if the identification response cannot be constructed
    fn received_vehicle_identification_request(
        &self,
        _client_info: &ClientConnectionInfo,
    ) -> Result<VehicleIdentificationResponse, Error> {
        Ok(VehicleIdentificationResponse {
            entity_id: self.get_entity_id(),
            logical_address: self.get_logical_address(),
            vin: self.get_vin(),
            group_id: self.get_group_id(),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        })
    }
    /// Identify vehicle by Entity ID (EID).
    /// Since the request includes the entity ID, my understanding is that only the vehicle in question should respond.
    /// The default implementation returns none if the request is not directed to the server in question
    ///
    /// # Errors
    /// Returns an [`Error`] if the identification response cannot be constructed
    fn vehicle_identification_with_eid(
        &self,
        _client_info: &ClientConnectionInfo,
        eid: &[u8; 6],
    ) -> Result<Option<VehicleIdentificationResponse>, Error> {
        if self.get_entity_id() == *eid {
            // If the request is directed to us, respond with our identification
            Ok(Some(VehicleIdentificationResponse {
                entity_id: self.get_entity_id(),
                logical_address: self.get_logical_address(),
                vin: self.get_vin(),
                group_id: self.get_group_id(),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }))
        } else {
            // This wasn't for us, so we don't have a response
            Ok(None)
        }
    }

    /// Identify vehicle by Vehicle Identification Number (VIN).
    /// Since the request includes the VIN, my understanding is that only the vehicle in question should respond.
    /// The default implementation returns none if the request is not directed to the server in question
    ///
    /// # Errors
    /// Returns an [`Error`] if the identification response cannot be constructed
    fn vehicle_identification_with_vin(
        &self,
        _client_info: &ClientConnectionInfo,
        vin: &[u8; 17],
    ) -> Result<Option<VehicleIdentificationResponse>, Error> {
        // If the request is directed to us, respond with our identification
        if self.get_vin() == *vin {
            Ok(Some(VehicleIdentificationResponse {
                entity_id: self.get_entity_id(),
                logical_address: self.get_logical_address(),
                vin: self.get_vin(),
                group_id: self.get_group_id(),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }))
        } else {
            // This wasn't for us, so we don't have a response
            Ok(None)
        }
    }

    /// Respond to an Alive Check request
    async fn alive_check(&self, client_info: &ClientConnectionInfo) -> Result<OwnedMessage, Error> {
        Ok(OwnedMessage::alive_check_response(
            self.protocol_version(),
            client_info.logical_address,
        ))
    }

    /// Respond to a diagnostic power mode information request
    /// Power mode is generally irrelevant for desktop applications,
    /// so the default implementation returns `NotSupported`
    async fn diagnostic_power_mode_information(
        &self,
        _client_info: &ClientConnectionInfo,
    ) -> Result<DiagnosticPowerModeCode, Error> {
        Ok(DiagnosticPowerModeCode::NotSupported)
    }

    /// The `DoIP` protocol version this entity reports in outgoing headers.
    /// Defaults to [`ProtocolVersion::V2012`] (ISO 13400-2:2012).
    fn protocol_version(&self) -> ProtocolVersion {
        ProtocolVersion::V2012
    }
}

/// A running `DoIP` entity: accepts TCP connections and dispatches incoming
/// messages to a [`ServerConnectionHandler`] implementation.
#[derive(Debug)]
pub struct Server<T> {
    connection_handler: Arc<T>,
    active_connections: AtomicUsize,
}

impl<T> Server<T>
where
    T: ServerConnectionHandler + Sync,
{
    /// Create a new `DoIP` server with the given connection handler
    ///
    /// # Errors
    /// Returns an [`Error`] if the server cannot be initialized
    pub fn new(connection_handler: T) -> Result<Self, Error> {
        // TODO: Validate the provided handler
        Ok(Server {
            connection_handler: Arc::new(connection_handler),
            active_connections: AtomicUsize::new(0),
        })
    }

    /// Start listening for incoming `DoIP` TCP connections
    ///
    /// # Errors
    /// Returns an [`Error`] if the TCP listener cannot be bound
    ///
    /// # Panics
    /// Panics if accepting a new TCP client connection fails
    pub async fn run_server(&self) -> Result<(), Error> {
        // TODO: Vehicle Announcement over UDP

        let tcp_listener = TcpListener::bind(("0.0.0.0", TCP_PORT)).await?;
        loop {
            match tcp_listener.accept().await {
                Ok((tcp_stream, client_socket_addr)) => {
                    if let Err(client_error) = self
                        .handle_client_connection(client_socket_addr, tcp_stream)
                        .await
                    {
                        error!("Client error: {client_error}");
                    }
                }
                Err(accept_error) => {
                    // TODO: Don't panic here, this might happen
                    panic!("Failed to accept new TCP client: {accept_error}");
                }
            }
        }
    }

    /// Handle an individual client TCP connection, reading and responding to messages
    ///
    /// # Errors
    /// Returns an [`Error`] if message handling or response encoding fails
    pub async fn handle_client_connection(
        &self,
        client_socket_addr: SocketAddr,
        tcp_stream: TcpStream,
    ) -> Result<(), Error> {
        let _active_connection_guard = ActiveConnectionGuard::new(&self.active_connections);
        let (rx, tx) = tcp_stream.into_split();
        let mut read_stream = FramedRead::new(rx, MessageCodec::new());
        let mut write_sink = FramedWrite::new(tx, MessageCodec::new());

        loop {
            match read_stream.next().await {
                Some(Ok(message)) => {
                    if let Some(response) = self
                        .handle_client_message(client_socket_addr, message)
                        .await?
                    {
                        write_sink.send(&response).await?;
                    }
                }
                Some(Err(codec_error)) => {
                    // A malformed header or codec error from a peer must not kill the task;
                    // log it and close this connection gracefully.
                    error!(
                        "Client decoding error, closing connection. source: {client_socket_addr}, {codec_error}"
                    );
                    return Ok(());
                }
                None => {
                    warn!("Client stream closed, client addr: {client_socket_addr}");
                    return Ok(());
                }
            }
        }
    }

    async fn handle_client_message(
        &self,
        client_socket_addr: SocketAddr,
        request_message: OwnedMessage,
    ) -> Result<Option<OwnedMessage>, Error> {
        // TODO: Need to handle active sockets by adding clients to a map
        // client count should come from that map, as well as the logical address missing below
        let connection_info = ClientConnectionInfo {
            ip_address: client_socket_addr.ip(),
            logical_address: LogicalAddress(0x0000), // TODO fix this constant
        };

        match request_message.payload {
            OwnedPayload::AliveCheckRequest => self
                .connection_handler
                .alive_check(&connection_info)
                .await
                .map(Some),
            OwnedPayload::DiagnosticMessage(diagnostic_message) => self
                .connection_handler
                .diagnostic_message(&diagnostic_message.as_ref())
                .await
                .map(Some),
            OwnedPayload::EntityStatusRequest => {
                warn!(
                    "Entity Status Request is not yet supported, ignoring. source: {client_socket_addr}"
                );
                Ok(None)
            }
            OwnedPayload::RoutingActivationRequest(request) => self
                .connection_handler
                .routing_activation(&request)
                .await
                .map(Some),
            OwnedPayload::RoutingActivationResponse(_routing_activation_response) => {
                warn!(
                    "Client sent a server-role RoutingActivationResponse message, source: {client_socket_addr}"
                );
                Err(Error::UnexpectedMessageType(
                    request_message.header.payload_type,
                ))
            }
            OwnedPayload::VehicleIdentificationRequest => {
                warn!(
                    "Vehicle Identification Request is not yet supported, ignoring. source: {client_socket_addr}"
                );
                Ok(None)
            }
            OwnedPayload::VehicleIdentificationResponse(_vehicle_identification_response) => {
                warn!(
                    "Client sent a server-role VehicleIdentificationResponse message, source: {client_socket_addr}"
                );
                Err(Error::UnexpectedMessageType(
                    request_message.header.payload_type,
                ))
            }
            _ => Err(Error::UnexpectedMessageType(
                request_message.header.payload_type,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveConnectionGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn guard_increments_on_creation_and_decrements_on_drop() {
        let active_connections = AtomicUsize::new(0);

        {
            let _guard = ActiveConnectionGuard::new(&active_connections);
            assert_eq!(active_connections.load(Ordering::Relaxed), 1);
        }

        assert_eq!(active_connections.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn guard_decrements_even_on_early_return_via_question_mark() {
        fn inner(active_connections: &AtomicUsize) -> Result<(), ()> {
            let _guard = ActiveConnectionGuard::new(active_connections);
            // Simulate an early return caused by `?` on some fallible operation,
            // e.g. `handle_client_message` returning `Err`.
            Err(())?;
            Ok(())
        }

        let active_connections = AtomicUsize::new(0);
        let result = inner(&active_connections);
        assert!(result.is_err());
        assert_eq!(active_connections.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn guard_decrements_on_panic_unwind() {
        let active_connections = AtomicUsize::new(0);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = ActiveConnectionGuard::new(&active_connections);
            panic!("simulated failure while holding the guard");
        }));

        assert!(result.is_err());
        assert_eq!(active_connections.load(Ordering::Relaxed), 0);
    }
}
