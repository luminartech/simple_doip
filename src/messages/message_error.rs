use crate::messages::{header::PayloadType, nack::NackCode};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DoIPMessageError {
    #[error("Negative acknowledgement: {0:?}")]
    Nack(NackCode),
    #[error("Version Inverse Incorrect: {value:?}")]
    VersionInverseIncorrect { value: u8 },
    #[error("Payload length in header does match expected payload type length: {value:?}, expected: {expected:?}")]
    PayloadLengthTooShort { value: usize, expected: u32 },
    #[error("Unexpected payload type found: {value:?}")]
    UnexpectedPayloadType { value: PayloadType },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
