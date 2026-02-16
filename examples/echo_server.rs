use async_trait::async_trait;
use simple_doip::{
    Error,
    logical_address::LogicalAddress,
    messages::{
        DiagnosticMessage, Message, RoutingActivationRequest, RoutingActivationResponseCode,
    },
    server::{Server, ServerConnectionHandler},
};
use tracing::{debug, info};

struct ServerHandler {}

#[async_trait]
impl ServerConnectionHandler for ServerHandler {
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
    ) -> Result<Message, Error> {
        info!(
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
    }

    async fn diagnostic_message(&self, message: &DiagnosticMessage) -> Result<Message, Error> {
        debug!(
            "Received diagnostic message from {:?} to {:?}",
            message.source_address, message.target_address
        );
        // Simply echo back the received data as raw bytes
        let response_data = message.user_data.clone();

        // Note: Using diagnostic_message() instead of diagnostic_message_ack()
        // - diagnostic_message() sends actual diagnostic data (the echoed payload)
        // - diagnostic_message_ack() would only send a transport-level acknowledgment
        // This creates a proper echo response rather than just acknowledging receipt
        Ok(Message::diagnostic_message(
            self.protocol_version(),
            message.source_address, // Keep original source as source
            message.target_address, // Keep original target as target
            response_data,
        ))
    }
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
