use doip::{
    client::{Client, ClientOptions},
    connection::ConnectorSocket,
    logical_address::LogicalAddress,
    messages::{ActivationTypeCode, ProtocolVersion},
    TCP_PORT, TESTER_LOGICAL_ADDRESS,
};
use std::net::{IpAddr, SocketAddr};
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting DOIP client");
    let local_server: IpAddr = "127.0.0.1"
        .parse()
        .expect("Hardcoded IP address should be valid");

    let custom_options = ClientOptions {
        server_address: SocketAddr::from((local_server, TCP_PORT)),
        server_logical_address: TESTER_LOGICAL_ADDRESS,
        server_physical_address: LogicalAddress(0x4010),
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: LogicalAddress(0x0E01),
        protocol_version: ProtocolVersion::V2012,
        routing_activation_options: Some(doip::client::RoutingActivationOptions {
            activation_type: ActivationTypeCode::Default,
            oem_specific: None,
        }),
        // 2 second interval recommended by UDS 14229-2 (S3client)
        tester_present_interval: std::time::Duration::from_secs(2),
        suppress_tester_present: true, // Suppress the reply from the server
    };

    let mut client = match Client::<ConnectorSocket>::connect(custom_options).await {
        Ok(client) => client,
        Err(e) => {
            info!("Failed to connect: {:?}", e);
            return Err(e.into());
        }
    };

    // Create a UDS Diagnostic Session Control request as raw bytes
    // Service ID: 0x10 (Diagnostic Session Control)
    // Session Type: 0x03 (Extended Diagnostic Session)
    let request_bytes = vec![0x10, 0x03];

    let resp = client
        .send_diagnostic_message(doip::client::AddressType::Physical, request_bytes)
        .await?;
    info!("Sent diagnostic message and received response {:#?}", resp);
    client.shut_down().await;
    Ok(())
}
