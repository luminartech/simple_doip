use async_trait::async_trait;
use simple_doip::{
    Error,
    logical_address::LogicalAddress,
    messages::{
        DiagnosticAckCode, DiagnosticMessage, OwnedMessage, RoutingActivationRequest,
        RoutingActivationResponseCode,
    },
    server::{ResponseWriter, Server, ServerConnectionHandler},
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
    ) -> Result<OwnedMessage, Error> {
        info!(
            "Routing activation request from {:?}",
            request.source_address
        );
        // 3.DoIP-090 NL
        // If source_address (SA) is not assigned to TCP_DATA sockets
        Ok(OwnedMessage::routing_activation_response(
            self.protocol_version(),
            request.source_address,
            self.get_logical_address(),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0; 4],
            None,
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), Error> {
        debug!(
            "Received diagnostic message from {:?} to {:?}",
            message.source_address, message.target_address
        );
        // A DoIP entity must acknowledge a diagnostic message first; any
        // functional (UDS) response is a separate, later DiagnosticMessage. Both
        // go through `responses`, in that order, which is the sequence a UDS
        // tester waits for.
        responses
            .send(OwnedMessage::diagnostic_message_ack(
                self.protocol_version(),
                message.target_address, // We are the target, so we answer as source
                message.source_address, // ...back to the tester that asked
                DiagnosticAckCode::RoutingConfirmationAck,
                message.user_data.to_vec(),
            ))
            .await?;
        // The echo itself: a real entity would put its UDS response here.
        responses
            .send(OwnedMessage::diagnostic_message(
                self.protocol_version(),
                message.target_address,
                message.source_address,
                message.user_data.to_vec(),
            ))
            .await?;
        Ok(())
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
