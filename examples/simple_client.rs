use std::net::{IpAddr, SocketAddr};

use async_trait::async_trait;
use doip::{
    client::{DoIPClient, DoIPClientOptions},
    messages::{
        diagnostic_message_ack::{DiagnosticAckCode, DiagnosticMessageAck},
        header::ProtocolVersion,
        routing_activation_request::ActivationTypeCode,
        routing_activation_response::{RoutingActivationResponse, RoutingActivationResponseCode},
    },
    server::{DoIPClientConnectionInfo, DoIPServer, DoIPServerConnectionHandler, SERVER_TCP_PORT},
    server_error::DoIPServerError,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = DoIPClientOptions {
        server_address: SocketAddr::from(([127, 0, 0, 1], SERVER_TCP_PORT)),
        server_logical_address: 0x0001,
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: 0x0E01,
        protocol_version: ProtocolVersion::V2012,
    };
    let mut client = DoIPClient::connect(options).await?;
    client
        .request_routing_activation(ActivationTypeCode::Default, None)
        .await?;
    client.request_alive_check().await?;
    client.close().await?;
    Ok(())
}
