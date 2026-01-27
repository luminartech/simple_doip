use crate::{
    logical_address::LogicalAddress,
    message_codec::MessageCodec,
    messages::{
        DiagnosticMessage, DiagnosticPowerModeCode, FurtherActionRequired, Message, Payload,
        ProtocolVersion, RoutingActivationRequest, VehicleIdentificationResponse, VinGidSyncStatus,
    },
    Error, TCP_PORT,
};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{error, warn};

pub struct ClientConnectionInfo {
    /// Client IP address
    pub ip_address: IpAddr,
    /// Client logical address
    pub logical_address: LogicalAddress,
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

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<Message, Error>;

    async fn diagnostic_message(&self, message: &DiagnosticMessage) -> Result<Message, Error>;

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
    async fn alive_check(&self, client_info: &ClientConnectionInfo) -> Result<Message, Error> {
        Ok(Message::alive_check_response(
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

    fn protocol_version(&self) -> ProtocolVersion {
        ProtocolVersion::V2012
    }
}

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
    ///
    /// # Panics
    /// Panics if a codec decoding error occurs on the client stream
    pub async fn handle_client_connection(
        &self,
        client_socket_addr: SocketAddr,
        tcp_stream: TcpStream,
    ) -> Result<(), Error> {
        let _currently_open_sockets = self.active_connections.fetch_add(1, Ordering::Relaxed);
        let (rx, tx) = tcp_stream.into_split();
        let mut read_stream = FramedRead::new(rx, MessageCodec::new());
        let mut write_sink = FramedWrite::new(tx, MessageCodec::new());

        loop {
            match read_stream.next().await {
                Some(Ok(message)) => {
                    let response = self
                        .handle_client_message(client_socket_addr, message)
                        .await?;

                    write_sink.send(&response).await?;
                }
                Some(Err(codec_error)) => {
                    panic!("Client, decoding error source: {client_socket_addr}, {codec_error}")
                }
                None => {
                    warn!("Client stream closed, client addr: {client_socket_addr}");
                    self.active_connections.fetch_sub(1, Ordering::Relaxed);
                    return Ok(());
                }
            }
        }
    }

    async fn handle_client_message(
        &self,
        client_socket_addr: SocketAddr,
        request_message: Message,
    ) -> Result<Message, Error> {
        // TODO: Need to handle active sockets by adding clients to a map
        // client count should come from that map, as well as the logical address missing below
        let connection_info = ClientConnectionInfo {
            ip_address: client_socket_addr.ip(),
            logical_address: LogicalAddress(0x0000), // TODO fix this constant
        };

        match request_message.payload {
            Payload::AliveCheckRequest => {
                self.connection_handler.alive_check(&connection_info).await
            }
            Payload::DiagnosticMessage(diagnostic_message) => {
                self.connection_handler
                    .diagnostic_message(&diagnostic_message)
                    .await
            }
            Payload::EntityStatusRequest => {
                todo!("Entity Status Request is not yet supported!")
            }
            Payload::RoutingActivationRequest(request) => {
                self.connection_handler.routing_activation(&request).await
            }
            Payload::RoutingActivationResponse(_routing_activation_response) => todo!(),
            Payload::VehicleIdentificationRequest => {
                todo!("Vehicle Identification Request is not yet supported")
            }
            Payload::VehicleIdentificationResponse(_vehicle_identification_response) => todo!(),
            _ => Err(Error::UnexpectedMessageType(
                request_message.header.payload_type,
            )),
        }
    }
}
