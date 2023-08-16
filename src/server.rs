use async_trait::async_trait;
use bincode::Error;

use crate::{
    client,
    messages::{
        alive_check_response::AliveCheckResponse,
        power_mode_info_response::DiagnosticPowerModeCode,
        routing_activation_request::ActivationTypeCode,
        routing_activation_response::RoutingActivationResponse,
        vehicle_identification_response::{
            FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
        },
    },
};
use std::net::{IpAddr, TcpListener};
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
#[async_trait]
pub trait DoIPServerConnectionHandler<ErrorType> {
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

    fn received_vehicle_identification_request(
        &self,
        client_info: &DoIPClientConnectionInfo,
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
    fn vehicle_identification_with_eid(
        &self,
        client_info: &DoIPClientConnectionInfo,
        eid: &[u8; 6],
    ) -> Result<Option<VehicleIdentificationResponse>, ErrorType> {
        if Self::get_entity_id() == *eid {
            Ok(Some(VehicleIdentificationResponse {
                entity_id: Self::get_entity_id(),
                logical_address: Self::get_logical_address(),
                vin: Self::get_vin(),
                group_id: Self::get_group_id(),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: VinGidSyncStatus::Synchronized,
            }))
        } else {
            Ok(None)
        }
    }

    /// Identify vehicle by Vehicle Identification Number (VIN).
    /// * `vin` - VIN as defined in ISO 3779.
    fn vehicle_identification_with_vin(
        &self,
        client_info: &DoIPClientConnectionInfo,
        vin: &[u8; 17],
    ) -> Result<Option<VehicleIdentificationResponse>, ErrorType> {
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
            Ok(None)
        }
    }

    async fn routing_activation(
        &self,
        client_info: &DoIPClientConnectionInfo,
        source_address: u16,
        activation_type: ActivationTypeCode,
    ) -> Result<RoutingActivationResponse, ErrorType>;

    async fn alive_check(
        &self,
        client_info: &DoIPClientConnectionInfo,
    ) -> Result<AliveCheckResponse, ErrorType> {
        Ok(AliveCheckResponse {
            source_address: client_info.logical_address,
        })
    }

    async fn diagnostic_power_mode_information(
        &self,
        client_info: &DoIPClientConnectionInfo,
    ) -> Result<DiagnosticPowerModeCode, ErrorType> {
        Ok(DiagnosticPowerModeCode::NotSupported)
    }
}

pub struct Server {
    tcp_listener: TcpListener,
}

impl Server {
    pub fn new(tls: bool) -> Self {
        let port = match tls {
            true => SERVER_TCP_TLS_PORT,
            false => SERVER_TCP_PORT,
        };
        let tcp_listener =
            TcpListener::bind(format!("127.0.0.1:{port}")).expect("Failed to bind to TCP port");
        Server { tcp_listener }
    }

    pub fn run(&self) {
        for stream in self.tcp_listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!("New connection: {}", stream.peer_addr().unwrap());
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
    }
}
