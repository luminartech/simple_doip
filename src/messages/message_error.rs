//! Error taxonomy for the `no_std` decode/encode core.
//!
//! [`MessageError`] variants fall along **two orthogonal axes**, not one flat tier
//! list:
//!
//! | Variant | Framing tier (per [`MessageError::is_framing_fatal`]) | Layer |
//! |---|---|---|
//! | `VersionInverseIncorrect` | framing-fatal | wire decode |
//! | `Incomplete` | framing-fatal (when surfaced by `try_frame`/`Decode`) | wire decode |
//! | `TrailingBytes` | recoverable | wire decode |
//! | `PayloadLengthTooShort`, `UnexpectedPayloadType`, `UnsupportedPayloadType` | recoverable (body) | wire decode |
//! | `Io` | recoverable (TX-side short write, e.g. `Io(WriteZero)` on an undersized stack buffer — S3) | encode / embedded-io |
//! | `Std` | not a frame property at all | tokio/codec boundary |

use crate::messages::header::PayloadType;

use automotive_wire_codec::{Incomplete, TrailingBytes};
use thiserror::Error;

/// Wire-level encode/decode failures from the `no_std` `DoIP` message core.
///
/// See the module-level table above for how each variant classifies along the
/// framing-fatal/recoverable axis (via [`MessageError::is_framing_fatal`]) and the
/// wire-decode/encode/transport layer axis.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MessageError {
    /// The header's inverse protocol version byte (ISO 13400-2 §7.1) did not match
    /// the bitwise complement of the protocol version byte. Since these two bytes
    /// are how a `DoIP` receiver validates frame sync, this is framing-fatal: the
    /// connection should be closed rather than the frame skipped.
    #[error("Version Inverse Incorrect, Expected: {expected:X}, got: {value:X}")]
    VersionInverseIncorrect {
        /// The inverse protocol version that should have been present, computed
        /// from the header's protocol version byte.
        expected: u8,
        /// The inverse protocol version byte actually found in the header.
        value: u8,
    },
    /// The header's `payload_length` field was smaller than the minimum size
    /// required for its declared `payload_type`, so the payload cannot contain a
    /// complete, well-formed body of that type.
    #[error(
        "Payload length in header does match expected payload type length: {value:?}, expected: {expected:?}"
    )]
    PayloadLengthTooShort {
        /// The payload length actually declared in the header.
        value: usize,
        /// The minimum payload length required for the header's `payload_type`.
        expected: u32,
    },
    /// A structurally valid, supported [`PayloadType`] was received in a context
    /// where it is not a legal response (e.g. a request-only type arriving as a
    /// response). Recoverable: the frame itself decoded fine.
    #[error("Unexpected payload type found: {0:?}")]
    UnexpectedPayloadType(PayloadType),
    /// The header declared a [`PayloadType`] this crate does not implement a
    /// decoder for. Recoverable: the frame's header and payload bytes are still
    /// available (via [`crate::try_frame`]) so the caller can skip/resync.
    #[error("Unsupported payload type, cannot decode: {0:?}")]
    UnsupportedPayloadType(PayloadType),
    /// `buf` did not contain enough bytes to decode the requested item. Surfaced
    /// framing-fatal when it comes from [`crate::try_frame`] or a top-level
    /// [`Decode`](crate::messages::Decode) call, since an incomplete header means
    /// stream sync cannot yet be trusted.
    #[error(transparent)]
    Incomplete(#[from] Incomplete),
    /// `buf` contained more bytes than the item being decoded consumed. Recoverable:
    /// framing itself succeeded, only the body was longer than expected.
    #[error(transparent)]
    TrailingBytes(#[from] TrailingBytes),
    /// An [`embedded_io`] write failed while encoding a message (e.g. `WriteZero`
    /// from an undersized stack buffer). Recoverable on the TX side: the caller can
    /// retry with a larger buffer.
    #[error("I/O error: {0:?}")]
    Io(embedded_io::ErrorKind),
    /// Full `std::io::Error` from the tokio/codec layer, preserving the OS error detail
    /// (code and message) that the flattened [`MessageError::Io`] kind would lose. The
    /// `no_std` core never produces this variant.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Std(std::io::Error),
}

impl MessageError {
    /// Framing-fatal errors mean stream sync is lost (header cannot be trusted);
    /// the client's only safe move is to close the connection. Everything else is
    /// frame-recoverable (NACK / ignore / skip).
    #[must_use]
    pub fn is_framing_fatal(&self) -> bool {
        matches!(
            self,
            MessageError::VersionInverseIncorrect { .. } | MessageError::Incomplete(_)
        )
    }
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
