use crate::messages::{header::PayloadType, nack::NackCode};

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MessageError {
    #[error("Negative acknowledgement: {0:?}")]
    Nack(NackCode),
    #[error("Version Inverse Incorrect, Expected: {expected:X}, got: {value:X}")]
    VersionInverseIncorrect { expected: u8, value: u8 },
    #[error(
        "Payload length in header does match expected payload type length: {value:?}, expected: {expected:?}"
    )]
    PayloadLengthTooShort { value: usize, expected: u32 },
    #[error("Unexpected payload type found: {0:?}")]
    UnexpectedPayloadType(PayloadType),
    #[error("Unsupported payload type, cannot decode: {0:?}")]
    UnsupportedPayloadType(PayloadType),
    #[error("Insufficient data: needed {needed} bytes, {available} available")]
    InsufficientData { needed: usize, available: usize },
    #[error("Trailing bytes after decode: {count}")]
    TrailingBytes { count: usize },
    #[error("I/O error: {0:?}")]
    Io(embedded_io::ErrorKind),
}

impl MessageError {
    /// Map any embedded-io error to [`MessageError::Io`].
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn io(err: impl embedded_io::Error) -> Self {
        MessageError::Io(err.kind())
    }
}

/// Required by `tokio_util::codec::Decoder` (its `Error` must be `From<std::io::Error>`).
#[cfg(feature = "std")]
impl From<std::io::Error> for MessageError {
    fn from(err: std::io::Error) -> Self {
        MessageError::Io(embedded_io::Error::kind(&err))
    }
}
