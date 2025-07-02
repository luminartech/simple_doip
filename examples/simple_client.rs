use doip::{
    client::{Client, ClientOptions},
    logical_address::LogicalAddress,
    messages::{ActivationTypeCode, ProtocolVersion},
    TCP_PORT, TESTER_LOGICAL_ADDRESS,
};
use std::net::{IpAddr, SocketAddr};
use tracing::info;
use tracing_subscriber;
use uds_protocol::{ProtocolRequest, ProtocolResponse};

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
    };

    let mut client = Client::<ProtocolResponse, ProtocolRequest>::connect(custom_options).await?;
    let port = client.bind_socket(custom_options.server_address).await?;

    info!("Bound to port: {}", port);

    let request = ProtocolRequest::diagnostic_session_control(
        false,
        uds_protocol::DiagnosticSessionType::ExtendedDiagnosticSession,
    );

    let resp = client
        .send_diagnostic_message(doip::client::AddressType::Physical, request)
        .await?;
    info!("Sent diagnostic message and received response {:#?}", resp);
    client.shut_down().await;
    Ok(())
}
