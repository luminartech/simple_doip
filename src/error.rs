use crate::{
    messages::{MessageError, NackCode, PayloadType},
    LogicalAddress,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    NetworkError(#[from] tokio::io::Error),
    #[error("Connection closed by remote host")]
    ConnectionClosed,
    #[error("Failed to send a message: {0}")]
    SendError(String),
    #[error(transparent)]
    MessageError(#[from] MessageError),
    #[error("Invalid logical address for client: {0:#06x}")]
    InvalidClientLogicalAddress(LogicalAddress),
    #[error(transparent)]
    ConnectionTimeout(#[from] tokio::time::error::Elapsed),
    #[error("Unexpected Ack message: {0:?}")]
    UnexpectedAckMessage(PayloadType),
    #[error("Unexpected message type: {0:?}")]
    UnexpectedMessageType(PayloadType),
    #[error(transparent)]
    ValueOutOfRange(#[from] std::num::TryFromIntError),
    #[error("Received Nack with code: {0:?}")]
    NackReceived(NackCode),
    #[error("DoIP Port 13400 expected, but got: {0}")]
    InvalidPort(u16),
    #[error("Socket not bound")]
    SocketNotBound,
    #[error("Socket closed unexpectedly")]
    SocketClosedUnexpectedly,
    #[error("Invalid client type")]
    InvalidClientType,
    #[error("Failed to bind to socket: {0}")]
    BindFailed(String),
    #[error("Failed to route activation")]
    RoutingActivationFailed,
    /// Request may have been suppressed, so this is a non-fatal error
    #[error("Response timeout exceeded")]
    ResponseTimeoutExceeded,
}
