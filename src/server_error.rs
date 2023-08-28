use crate::messages::{header::PayloadType, message_error::DoIPMessageError};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DoIPServerError {
    #[error(transparent)]
    NetworkError(#[from] tokio::io::Error),
    #[error(transparent)]
    MessageError(#[from] DoIPMessageError),
    #[error("Unsupported message type: {0:?}")]
    UnsupportedMessageTypeError(PayloadType),
}
