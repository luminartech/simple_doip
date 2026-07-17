use crate::messages::{header::PayloadType, nack::NackCode};

use automotive_wire_codec::{Incomplete, TrailingBytes};
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
    #[error(transparent)]
    Incomplete(#[from] Incomplete),
    #[error(transparent)]
    TrailingBytes(#[from] TrailingBytes),
    #[error("I/O error: {0:?}")]
    Io(embedded_io::ErrorKind),
    /// Full `std::io::Error` from the tokio/codec layer, preserving the OS error detail
    /// (code and message) that the flattened [`MessageError::Io`] kind would lose. The
    /// `no_std` core never produces this variant.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Std(std::io::Error),
}

impl From<embedded_io::ErrorKind> for MessageError {
    fn from(kind: embedded_io::ErrorKind) -> Self {
        MessageError::Io(kind)
    }
}

/// Required by `tokio_util::codec::Decoder` (its `Error` must be `From<std::io::Error>`).
///
/// Preserves the full [`std::io::Error`] (OS code and message) rather than flattening to
/// an [`embedded_io::ErrorKind`], so the tokio layer keeps actionable error detail.
#[cfg(feature = "std")]
impl From<std::io::Error> for MessageError {
    fn from(err: std::io::Error) -> Self {
        MessageError::Std(err)
    }
}
