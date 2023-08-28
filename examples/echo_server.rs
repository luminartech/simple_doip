use async_trait::async_trait;
use doip::{
    messages::{
        diagnostic_message_ack::{DiagnosticAckCode, DiagnosticMessageAck},
        routing_activation_request::ActivationTypeCode,
        routing_activation_response::{RoutingActivationResponse, RoutingActivationResponseCode},
    },
    server::{DoIPClientConnectionInfo, DoIPServer, DoIPServerConnectionHandler},
    server_error::DoIPServerError,
};

struct ServerHandler {}

#[async_trait]
impl DoIPServerConnectionHandler<DoIPServerError> for ServerHandler {
    fn get_vin() -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address() -> u16 {
        0x0001
    }

    fn get_entity_id() -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id() -> Option<[u8; 6]> {
        None
    }

    ///
    async fn routing_activation(
        &self,
        _client_info: &DoIPClientConnectionInfo,
        source_address: u16,
        _activation_type: ActivationTypeCode,
    ) -> Result<RoutingActivationResponse, DoIPServerError> {
        Ok(RoutingActivationResponse {
            logical_address_tester: source_address,
            logical_address_of_doip_entity: 0x0001,
            routing_activation_response_code:
                RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            reserved_oem: [0x00, 0x00, 0x00, 0x00],
            oem_specific: Some([0, 0, 0, 0]),
        })
    }

    async fn diagnostic_message(
        &self,
        _client_info: &DoIPClientConnectionInfo,
        source_address: u16,
        target_address: u16,
        user_data: Vec<u8>,
    ) -> Result<DiagnosticMessageAck, DoIPServerError> {
        Ok(DiagnosticMessageAck {
            source_address: source_address,
            target_address: target_address,
            ack_code: DiagnosticAckCode::RoutingConfirmationAck,
            previous_message_data: user_data,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let handler = ServerHandler {};
    let server = DoIPServer::new(handler)?;

    server.run_server().await?;

    Ok(())
}
