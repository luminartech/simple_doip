use std::net::{IpAddr, SocketAddr};

use doip::{
    client::{Client, ClientOptions},
    logical_address::LogicalAddress,
    messages::{ActivationTypeCode, ProtocolVersion},
    LIDAR_LOGICAL_ADDRESS, TCP_PORT,
};
use uds_protocol::{ProtocolRequest, ProtocolResponse};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = ClientOptions {
        server_address: SocketAddr::from(([127, 0, 0, 1], TCP_PORT)),
        server_logical_address: LIDAR_LOGICAL_ADDRESS,
        server_physical_address: LogicalAddress(0x4010),
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: LogicalAddress(0x0E01),
        protocol_version: ProtocolVersion::V2012,
    };
    let mut client = Client::<ProtocolRequest, ProtocolResponse>::connect(options).await?;
    client
        .request_routing_activation(ActivationTypeCode::Default, None)
        .await?;
    client.shut_down().await;
    Ok(())
}
