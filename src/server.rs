use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;

use crate::{
    message_codec::DoIPMessageCodec,
    messages::{
        alive_check_response::AliveCheckResponse,
        diagnostic_message_ack::DiagnosticMessageAck,
        entity_status_response::{EntityStatusNodeType, EntityStatusResponse},
        header::{DoIPHeader, PayloadType, ProtocolVersion},
        power_mode_info_response::DiagnosticPowerModeCode,
        routing_activation_request::{ActivationTypeCode, RoutingActivationRequest},
        routing_activation_response::RoutingActivationResponse,
        vehicle_identification_response::{
            FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
        },
        DoIPMessage,
    },
    server_error::DoIPServerError,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Default TCP port for DoIP
/// This is the port used for unencrypted connections
pub const SERVER_TCP_PORT: u16 = 13400;

/// TODO: Implement TLS support
pub const SERVER_TCP_TLS_PORT: u16 = 3496;

pub struct DoIPClientConnectionInfo {
    /// Client IP address
    pub ip_address: IpAddr,
    /// Client logical address
    pub logical_address: u16,
}
/// Trait for handling DoIP connections as a server.
/// Implement this trait to create a custom DoIP server.
/// Most protocol functions have a simple, default implementation
#[async_trait]
pub trait DoIPServerConnectionHandler<ErrorType> {
    // Required Functions
    // These functions must be implemented by the server implementation

    /// Get the Vehicle Identification Number for this server
    fn get_vin() -> [u8; 17];

    /// Get the ECU logical address for this server
    fn get_logical_address() -> u16;

    /// Get the unique entity ID for this server
    /// This is usually the MAC address of the network interface.
    fn get_entity_id() -> [u8; 6];

    /// Get the unique group identification
    /// Optional field, return `None` if not set.
    fn get_group_id() -> Option<[u8; 6]>;

    async fn routing_activation(
        &self,
        client_info: &DoIPClientConnectionInfo,
        source_address: u16,
        activation_type: ActivationTypeCode,
    ) -> Result<RoutingActivationResponse, ErrorType>;

    async fn diagnostic_message(
        &self,
        client_info: &DoIPClientConnectionInfo,
        source_address: u16,
        target_address: u16,
        user_data: Vec<u8>,
    ) -> Result<DiagnosticMessageAck, ErrorType>;

    // Optional Functions
    // These functions *may* be overridden to provide custom behavior
    // Default functionality is very simplistic and may not be suitable for production use

    /// Respond to an Identification request with the identity parameters provided by the trait implementer
    fn received_vehicle_identification_request(
        &self,
        _client_info: &DoIPClientConnectionInfo,
    ) -> Result<VehicleIdentificationResponse, ErrorType> {
        Ok(VehicleIdentificationResponse {
            entity_id: Self::get_entity_id(),
            logical_address: Self::get_logical_address(),
            vin: Self::get_vin(),
            group_id: Self::get_group_id(),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: VinGidSyncStatus::Synchronized,
        })
    }
    /// Identify vehicle by Entity ID (EID).
    /// Since the request includes the entity ID, my understanding is that only the vehicle in question should respond.
    /// The default implementation returns none if the request is not directed to the server in question
    fn vehicle_identification_with_eid(
        &self,
        _client_info: &DoIPClientConnectionInfo,
        eid: &[u8; 6],
    ) -> Result<Option<VehicleIdentificationResponse>, ErrorType> {
        if Self::get_entity_id() == *eid {
            // If the request is directed to us, respond with our identification
            Ok(Some(VehicleIdentificationResponse {
                entity_id: Self::get_entity_id(),
                logical_address: Self::get_logical_address(),
                vin: Self::get_vin(),
                group_id: Self::get_group_id(),
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
    fn vehicle_identification_with_vin(
        &self,
        _client_info: &DoIPClientConnectionInfo,
        vin: &[u8; 17],
    ) -> Result<Option<VehicleIdentificationResponse>, ErrorType> {
        // If the request is directed to us, respond with our identification
        if Self::get_vin() == *vin {
            Ok(Some(VehicleIdentificationResponse {
                entity_id: Self::get_entity_id(),
                logical_address: Self::get_logical_address(),
                vin: Self::get_vin(),
                group_id: Self::get_group_id(),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }))
        } else {
            // This wasn't for us, so we don't have a response
            Ok(None)
        }
    }
    /// Respond to an Alive Check request
    async fn alive_check(
        &self,
        client_info: &DoIPClientConnectionInfo,
    ) -> Result<AliveCheckResponse, ErrorType> {
        Ok(AliveCheckResponse {
            source_address: client_info.logical_address,
        })
    }

    /// Respond to a diagnostic power mode information request
    /// Power mode is generally irrelevant for desktop applications,
    /// so the default implementation returns `NotSupported`
    async fn diagnostic_power_mode_information(
        &self,
        _client_info: &DoIPClientConnectionInfo,
    ) -> Result<DiagnosticPowerModeCode, ErrorType> {
        Ok(DiagnosticPowerModeCode::NotSupported)
    }
}

pub struct DoIPServer<T: DoIPServerConnectionHandler<DoIPServerError>> {
    connection_handler: Arc<T>,
    active_connections: AtomicUsize,
}

impl<T: DoIPServerConnectionHandler<DoIPServerError> + std::marker::Sync> DoIPServer<T> {
    pub fn new(connection_handler: T) -> Result<Self, DoIPServerError> {
        // TODO: Validate the provided handler
        Ok(DoIPServer {
            connection_handler: Arc::new(connection_handler),
            active_connections: AtomicUsize::new(0),
        })
    }

    pub async fn run_server(&self) -> Result<(), DoIPServerError> {
        // TODO: Vehicle Announcement over UDP

        let tcp_listener = TcpListener::bind(("0.0.0.0", SERVER_TCP_PORT)).await?;
        loop {
            match tcp_listener.accept().await {
                Ok((tcp_stream, client_socket_addr)) => {
                    if let Err(client_error) = self
                        .handle_client_connection(client_socket_addr, tcp_stream)
                        .await
                    {
                        println!("Client error: {client_error}");
                    }
                }
                Err(accept_error) => {
                    // TODO: Don't panic here, this might happen
                    panic!("Failed to accept new TCP client: {accept_error}");
                }
            }
        }
    }

    async fn handle_client_connection(
        &self,
        client_socket_addr: SocketAddr,
        tcp_stream: TcpStream,
    ) -> Result<(), DoIPServerError> {
        let _currently_open_sockets = self.active_connections.fetch_add(1, Ordering::Relaxed);

        let mut client_message_stream = Framed::new(tcp_stream, DoIPMessageCodec {});

        loop {
            match client_message_stream.next().await {
                Some(Ok(message)) => {
                    let response = self
                        .handle_client_message(client_socket_addr, message)
                        .await?;

                    client_message_stream.send(&response).await?;
                }
                Some(Err(codec_error)) => {
                    panic!("Client, decoding error source: {client_socket_addr}, {codec_error}")
                }
                None => {
                    println!("Client stream closed, client addr: {client_socket_addr}");
                    self.active_connections.fetch_sub(1, Ordering::Relaxed);
                    return Ok(());
                }
            }
        }
    }

    async fn handle_client_message(
        &self,
        client_socket_addr: SocketAddr,
        message: DoIPMessage,
    ) -> Result<DoIPMessage, DoIPServerError> {
        // TODO: Need to handle active sockets by adding clients to a map
        // client count should come from that map, as well as the logical address missing below
        let connection_info = DoIPClientConnectionInfo {
            ip_address: client_socket_addr.ip(),
            logical_address: 0x0000, // TODO fix this constant
        };
        let mut response_payload = Vec::new();
        let response: Result<(PayloadType, Vec<u8>), DoIPServerError> = match message
            .header
            .payload_type
        {
            PayloadType::AliveCheckRequest => {
                let response = self
                    .connection_handler
                    .alive_check(&connection_info)
                    .await?;
                response.write(&mut response_payload)?;

                Ok((PayloadType::AliveCheckResponse, response_payload))
            }
            PayloadType::RoutingActivationRequest => {
                let request = RoutingActivationRequest::read(&mut message.payload.as_slice())?;
                let source_address = request.source_address;
                let response = self
                    .connection_handler
                    .routing_activation(&connection_info, source_address, request.activation_type)
                    .await?;

                response.write(&mut response_payload)?;

                Ok((PayloadType::RoutingActivationResponse, response_payload))
            }
            PayloadType::DoIPEntityStatusRequest => {
                let response = EntityStatusResponse {
                    node_type: EntityStatusNodeType::DoIPNode,
                    max_concurrent_tcp_sockets: u8::MAX,
                    open_tcp_sockets: self.active_connections.load(Ordering::Relaxed) as u8,
                    max_data_size: u32::MAX,
                };
                response.write(&mut response_payload)?;
                Ok((PayloadType::DoIPEntityStatusResponse, response_payload))
            }
            // TODO add remaining
            _ => Err(DoIPServerError::UnsupportedMessageTypeError(
                message.header.payload_type,
            )),
        };

        let (payload_type, payload) = response?;

        let header = DoIPHeader::new(ProtocolVersion::V2012, payload_type, payload.len() as u32);
        Ok(DoIPMessage { header, payload })
    }
}
