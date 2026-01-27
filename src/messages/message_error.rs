use crate::messages::{header::PayloadType, nack::NackCode};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MessageError {
    #[error("Negative acknowledgement: {0:?}")]
    Nack(NackCode),
    #[error("Version Inverse Incorrect, Expected: {expected:X}, got: {value:X}")]
    VersionInverseIncorrect { expected: u8, value: u8 },
    #[error("Payload length in header does match expected payload type length: {value:?}, expected: {expected:?}")]
    PayloadLengthTooShort { value: usize, expected: u32 },
    #[error("Unexpected payload type found: {0:?}")]
    UnexpectedPayloadType(PayloadType),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
