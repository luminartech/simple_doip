use crate::{
    client_error::DoIPClientError,
    message_codec::DoIPMessageCodec,
    messages::{
        alive_check_response::AliveCheckResponse,
        diagnostic_message::DiagnosticMessage,
        diagnostic_message_ack::DiagnosticMessageAck,
        header::{DoIPHeader, PayloadType, ProtocolVersion},
        nack::NackCode,
        routing_activation_request::{ActivationTypeCode, RoutingActivationRequest},
        routing_activation_response::RoutingActivationResponse,
        DoIPMessage,
    },
};

use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, SocketAddr};
use tokio::net::{TcpSocket, TcpStream};
use tokio_util::codec::Framed;

/// DoIP client options used to specify connection info
/// Derive `Serialize` and `Deserialize` for use in config files
#[derive(Debug)]
pub struct DoIPClientOptions {
    /// Server IP address and port
    pub server_address: SocketAddr,
    /// Target logical addresses, uniquely identifies the ECU to be diagnosed.
    /// Valid range: 0x0001 - 0x0DFF
    pub server_logical_address: u16,
    /// Local ip address to bind the TCP and UDP sockets to, e.g. `0.0.0.0`. The port is randomly chosen.
    pub client_address: IpAddr,
    /// Valid range: 0x0E00 - 0x0FFF
    pub client_logical_address: u16,
    /// Which protocol version the client should
    pub protocol_version: ProtocolVersion,
}

pub struct DoIPClient {
    client_options: DoIPClientOptions,
    tcp_stream: Framed<TcpStream, DoIPMessageCodec>,
}

impl DoIPClient {
    /// Create a DoIP connection.
    /// The target port defaults to [`SERVER_TCP_PORT`].
    pub async fn connect(client_options: DoIPClientOptions) -> Result<Self, DoIPClientError> {
        if client_options.client_logical_address < 0x0E00
            || client_options.client_logical_address > 0x0FFF
        {
            return Err(DoIPClientError::InvalidClientLogicalAddress(
                client_options.client_logical_address,
            ));
        }

        let tcp_socket = match client_options.server_address {
            SocketAddr::V4(_) => TcpSocket::new_v4()?,
            SocketAddr::V6(_) => TcpSocket::new_v6()?,
        };
        tcp_socket.set_reuseaddr(true)?;

        let tcp_stream = tcp_socket.connect(client_options.server_address).await?;
        let framed_stream = Framed::new(tcp_stream, DoIPMessageCodec {});

        Ok(Self {
            client_options,
            tcp_stream: framed_stream,
        })
    }

    pub async fn close(&mut self) -> Result<(), DoIPClientError> {
        self.tcp_stream.flush().await?;
        self.tcp_stream.close().await?;
        Ok(())
    }

    pub async fn request_routing_activation(
        &mut self,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Result<RoutingActivationResponse, DoIPClientError> {
        let request = RoutingActivationRequest {
            source_address: self.client_options.client_logical_address,
            activation_type,
            reserved: [0, 0, 0, 0],
            reserved_vehicle_manufacturer,
        };

        let mut payload = Vec::with_capacity(11);
        request.write(&mut payload)?;

        let header = DoIPHeader::new(
            self.client_options.protocol_version,
            PayloadType::RoutingActivationRequest,
            payload.len().try_into()?,
        );
        let message = DoIPMessage { header, payload };
        self.tcp_stream.send(&message).await?;

        let response_message = self.read_tcp_message().await?;
        if response_message.header.payload_type != PayloadType::RoutingActivationResponse {
            return Err(DoIPClientError::UnexpectedMessageType(
                message.header.payload_type,
            ));
        }

        let response_payload = RoutingActivationResponse::read(
            &mut response_message.payload.as_slice(),
            response_message.header.payload_length as usize,
        )?;
        Ok(response_payload)
    }

    pub async fn request_alive_check(&mut self) -> Result<AliveCheckResponse, DoIPClientError> {
        let header = DoIPHeader::new(
            self.client_options.protocol_version,
            PayloadType::AliveCheckRequest,
            0,
        );
        let message = DoIPMessage {
            header,
            payload: Vec::new(),
        };
        self.tcp_stream.send(&message).await?;

        let response_message = self.read_tcp_message().await?;

        if response_message.header.payload_type != PayloadType::AliveCheckResponse {
            return Err(DoIPClientError::UnexpectedMessageType(
                response_message.header.payload_type,
            ));
        }

        let response_payload = AliveCheckResponse::read(&mut response_message.payload.as_slice())?;
        Ok(response_payload)
    }

    pub async fn diagnostic_message(
        &mut self,
        source_address: u16,
        target_address: u16,
        user_data: Vec<u8>,
    ) -> Result<DiagnosticMessageAck, DoIPClientError> {
        let diagnostic_message = DiagnosticMessage {
            source_address,
            target_address,
            user_data,
        };

        let mut payload = Vec::with_capacity(4 + diagnostic_message.user_data.len());
        diagnostic_message.write(&mut payload)?;

        let header = DoIPHeader::new(
            self.client_options.protocol_version,
            PayloadType::DiagnosticMessage,
            payload.len().try_into()?,
        );
        let message = DoIPMessage { header, payload };
        self.tcp_stream.send(&message).await?;
        let response_message = self.read_tcp_message().await?;

        match response_message.header.payload_type {
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                let ack = DiagnosticMessageAck::read(
                    &mut response_message.payload.as_slice(),
                    response_message.header.payload_length as usize,
                )?;
                Ok(ack)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => {
                let nack = DiagnosticMessageAck::read(
                    &mut message.payload.as_slice(),
                    message.header.payload_length as usize,
                )?;
                // I don't think we want to error here, let the caller handle the nack
                Ok(nack)
            }
            _ => Err(DoIPClientError::UnexpectedMessageType(
                message.header.payload_type,
            )),
        }
    }

    async fn read_tcp_message(&mut self) -> Result<DoIPMessage, DoIPClientError> {
        // Unwrap here is to unwrap the option, not the result
        let message = self.tcp_stream.next().await.unwrap()?;

        if message.header.payload_type == PayloadType::NegativeAcknowledge {
            let nack_code = NackCode::read(&mut message.payload.as_slice())?;
            return Err(DoIPClientError::NackReceived(nack_code));
        }

        Ok(message)
    }
}
