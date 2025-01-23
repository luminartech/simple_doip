use crate::messages::{DoIPMessageError, NackCode, PayloadType};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    NetworkError(#[from] tokio::io::Error),
    #[error(transparent)]
    MessageError(#[from] DoIPMessageError),
    #[error("Invalid logical address for client: {0:#06x}")]
    InvalidClientLogicalAddress(u16),
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
}
