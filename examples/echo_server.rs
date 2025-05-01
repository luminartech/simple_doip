use async_trait::async_trait;
use doip::{
    logical_address::LogicalAddress,
    messages::{
        DiagnosticAckCode, DiagnosticMessage, Message, RoutingActivationRequest,
        RoutingActivationResponseCode,
    },
    server::{Server, ServerConnectionHandler},
    Error,
};
use tracing::debug;
use uds_protocol::{ProtocolRequest, ProtocolResponse, WireFormat};

struct ServerHandler {}

#[async_trait]
impl ServerConnectionHandler<ProtocolRequest, ProtocolResponse> for ServerHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        LogicalAddress(0x0001)
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<Message<ProtocolResponse>, Error> {
        println!(
            "Routing activation request from {:?}",
            request.source_address
        );
        // 3.DoIP-090 NL
        // If source_address (SA) is not assigned to TCP_DATA sockets
        Ok(Message::routing_activation_response(
            self.protocol_version(),
            request.source_address,
            self.get_logical_address(),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0; 4],
            None,
        ))
        // TODO: Much of this server code is temporarily disabled until we re-work the traits for DoIP entities
        /*let response = {
            Ok(RoutingActivationResponse {
                logical_address_tester: request.source_address,
                logical_address_of_doip_entity: 0x0001,
                routing_activation_response_code:
                    RoutingActivationResponseCode::RoutingSuccessfullyActivated,
                reserved_oem: [0x00, 0x00, 0x00, 0x00],
                oem_specific: Some([0, 0, 0, 0]),
            })
        };
        Ok(RoutingActivationResponse {
            logical_address_tester: source_address,
            logical_address_of_doip_entity: 0x0001,
            routing_activation_response_code:
                RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            reserved_oem: [0x00, 0x00, 0x00, 0x00],
            oem_specific: Some([0, 0, 0, 0]),
        })*/
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<ProtocolRequest>,
    ) -> Result<Message<ProtocolResponse>, Error> {
        debug!(
            "Received diagnostic message from {:?} to {:?}",
            message.source_address, message.target_address
        );
        let mut previous_message_data = Vec::with_capacity(message.user_data.required_size());
        message
            .user_data
            .to_writer(&mut previous_message_data)
            .unwrap();
        Ok(Message::diagnostic_message_ack(
            self.protocol_version(),
            message.source_address,
            message.target_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            previous_message_data,
        ))
    }
    /*
    ///
    async fn routing_activation(
        &self,
        _client_info: &ClientConnectionInfo,
        source_address: u16,
        _activation_type: ActivationTypeCode,
    ) -> Result<RoutingActivationResponse, Error>

    async fn diagnostic_message(
        &self,
        _client_info: &ClientConnectionInfo,
        source_address: u16,
        target_address: u16,
        user_data: Vec<u8>,
    ) -> Result<DiagnosticMessageAck, Error> {
        Ok(DiagnosticMessageAck {
            source_address: source_address,
            target_address: target_address,
            ack_code: DiagnosticAckCode::RoutingConfirmationAck,
            previous_message_data: user_data,
        })
    }
    */
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let handler = ServerHandler {};
    let server = Server::new(handler)?;

    server.run_server().await?;

    Ok(())
}
