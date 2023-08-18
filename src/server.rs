use async_trait::async_trait;
use tokio::net::UdpSocket;

use crate::{
    messages::{
        alive_check_response::AliveCheckResponse,
        diagnostic_message_ack::DiagnosticMessageAck,
        power_mode_info_response::DiagnosticPowerModeCode,
        routing_activation_request::ActivationTypeCode,
        routing_activation_response::RoutingActivationResponse,
        vehicle_identification_response::{
            FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
        },
    },
    server_error::DoIPServerError,
};
use std::{
    net::{IpAddr, SocketAddr, TcpListener},
    sync::Arc,
};
const SERVER_TCP_PORT: u16 = 13400;
const SERVER_TCP_TLS_PORT: u16 = 3496;

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

    fn get_client_udp_address() -> SocketAddr;

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
    ///
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
    // These functions *may* be overriden to provide custom behavior
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
}

impl<T: DoIPServerConnectionHandler<DoIPServerError> + std::marker::Sync> DoIPServer<T> {
    pub fn new(tls: bool, connection_handler: T) -> Self {
        let port = match tls {
            true => SERVER_TCP_TLS_PORT,
            false => SERVER_TCP_PORT,
        };
        let tcp_listener =
            TcpListener::bind(format!("127.0.0.1:{port}")).expect("Failed to bind to TCP port");
        DoIPServer {
            connection_handler: Arc::new(connection_handler),
        }
    }

    pub async fn run(&self) -> Result<(), DoIPServerError> {
        let target_address =
            <T as DoIPServerConnectionHandler<DoIPServerError>>::get_client_udp_address();
        let udp = UdpSocket::bind(target_address).await?;
        Ok(())
        /* /
        //let udp = UdpSocket::bind("
        // Tokio's UdpSocket does not directly offer "set_reuse_address", go with socket2
        let udp_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        udp_socket.set_reuse_address(true)?;
        udp_socket.set_broadcast(true)?;

        // The port zero indicates that a random, free port is chosen.
        let client_addr_udp = SocketAddr::new(self.addr, 0);
        udp_socket.bind(&client_addr_udp.into())?;
        let udp_socket = UdpSocket::from_std(udp_socket.into())?;

        let listener = TcpListener::bind(("0.0.0.0", TCP_DATA_TLS_PORT)).await?;

        loop {
            match listener.accept().await {
                Ok((tcp_stream, client_socket_addr)) => {
                    if let Err(client_error) =
                        self.handle_client(client_socket_addr, tcp_stream).await
                    {
                        error!("Error occured: {client_error}");
                    }
                }
                Err(accept_error) => {
                    error!("Failed to accept new TCP client: {accept_error}");
                }
            }
        }
        */
    }
}
