use crate::messages::{header::PayloadType, message_error::DoIPMessageError, nack::NackCode};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DoIPClientError {
    #[error(transparent)]
    NetworkError(#[from] tokio::io::Error),
    #[error(transparent)]
    MessageError(#[from] DoIPMessageError),
    #[error("Invalid logical address for client: {0:#06x}")]
    InvalidClientLogicalAddress(u16),
    #[error("Unexpected Ack message: {0:?}")]
    UnexpectedAckMessage(PayloadType),
    #[error("Unexpected message type: {0:?}")]
    UnexpectedMessageType(PayloadType),
    #[error(transparent)]
    ValueOutOfRange(#[from] std::num::TryFromIntError),
    #[error("Received Nack with code: {0:?}")]
    NackReceived(NackCode),
}
