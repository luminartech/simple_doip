use crate::{
    LogicalAddress,
    messages::{DiagnosticAckCode, MessageError, NackCode, PayloadType},
};
use std::string::String;
use thiserror::Error;

/// Errors surfaced by the client and server connection machinery (as opposed to
/// [`MessageError`], which covers wire-level encode/decode failures).
///
/// `Error` is `#[non_exhaustive]`: new variants may be added in a semver-compatible
/// release, so downstream `match` expressions must include a wildcard arm.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The underlying TCP/UDP socket returned an I/O error (e.g. the OS reported a
    /// broken pipe or reset connection) while sending or receiving `DoIP` traffic.
    #[error(transparent)]
    NetworkError(#[from] tokio::io::Error),
    /// The `DoIP` entity or the connection task on the other end of an internal
    /// channel closed the connection before a request could be answered.
    #[error("Connection closed by remote host")]
    ConnectionClosed,
    /// A message could not be handed off to the connection's write half; the
    /// contained string describes the underlying I/O or channel failure.
    #[error("Failed to send a message: {0}")]
    SendError(String),
    /// Wraps a wire-level encode/decode failure (malformed header, unsupported
    /// payload type, etc.) from [`MessageError`].
    #[error(transparent)]
    MessageError(#[from] MessageError),
    /// The client attempted to use a [`LogicalAddress`] outside the tester range
    /// `0x0E00`-`0x0FFF`
    /// (see [`LogicalAddress::is_valid_client_address`]).
    #[error("Invalid logical address for client: {0:#06x}")]
    InvalidClientLogicalAddress(LogicalAddress),
    /// A request did not receive a response before its deadline elapsed (e.g. the
    /// `A_DoIP_Diagnostic_Message` response timeout).
    #[error(transparent)]
    ConnectionTimeout(#[from] tokio::time::error::Elapsed),
    /// An ACK/NACK payload type was received where the caller was not expecting one.
    ///
    /// Currently never produced by this crate.
    #[error("Unexpected Ack message: {0:?}")]
    UnexpectedAckMessage(PayloadType),
    /// A received `DoIP` payload's type did not match the type expected for the
    /// current protocol state (e.g. a routing activation response arrived while
    /// waiting on a diagnostic message ACK).
    #[error("Unexpected message type: {0:?}")]
    UnexpectedMessageType(PayloadType),
    /// A numeric conversion between wire and native integer widths overflowed
    /// (e.g. a payload length or count that does not fit the target type).
    ///
    /// Currently never produced by this crate.
    #[error(transparent)]
    ValueOutOfRange(#[from] std::num::TryFromIntError),
    /// The `DoIP` entity responded to a request with a `DoIP` NACK; the contained
    /// [`NackCode`] identifies the reported reason.
    ///
    /// Currently never produced by this crate.
    #[error("Received Nack with code: {0:?}")]
    NackReceived(NackCode),
    /// A gateway address was supplied with a TCP port other than the standard
    /// `DoIP` port `13400` (see [`crate::TCP_PORT`]).
    #[error("DoIP Port 13400 expected, but got: {0}")]
    InvalidPort(u16),
    /// An operation requiring a bound socket was attempted before
    /// [`Client::connect`](crate::client::Client::connect) (or the equivalent
    /// server setup) completed, or after the socket was unbound.
    #[error("Socket not bound")]
    SocketNotBound,
    /// The connection's socket was closed while a response was still expected,
    /// with no `DoIP` message indicating why.
    #[error("Socket closed unexpectedly")]
    SocketClosedUnexpectedly,
    /// The requested client configuration does not match a supported `DoIP`
    /// client type.
    ///
    /// Currently never produced by this crate.
    #[error("Invalid client type")]
    InvalidClientType,
    /// Binding the UDP/TCP socket to the requested local address failed; the
    /// contained string describes the underlying OS error.
    #[error("Failed to bind to socket: {0}")]
    BindFailed(String),
    /// The `DoIP` entity did not grant routing activation (see
    /// [`RoutingActivationResponse`](crate::messages::RoutingActivationResponse)),
    /// so diagnostic messages cannot yet be exchanged on this connection.
    #[error("Failed route activation")]
    RoutingActivationFailed,
    /// No response arrived within `A_DoIP_Diagnostic_Message`.
    /// The request may simply have been suppressed by the target ECU per the
    /// diagnostic addressing rules, so this is treated as non-fatal rather than a
    /// hard protocol violation.
    #[error("Response timeout exceeded")]
    ResponseTimeoutExceeded,
    /// A request was still pending when a new request was issued on the same
    /// client, so the older request was superseded and will never complete.
    ///
    /// This is what a caller observes if it stops awaiting a request (for
    /// example by wrapping it in [`tokio::time::timeout`] or racing it in a
    /// `tokio::select!`) and then issues another request: the abandoned request
    /// is displaced by the new one rather than being dropped silently.
    #[error("Request superseded by a newer request on the same client")]
    RequestSuperseded,
    /// The `DoIP` entity rejected a diagnostic message with a negative
    /// `DiagnosticMessageAck`; the contained [`DiagnosticAckCode`] identifies the
    /// reported reason.
    #[error("Diagnostic message NACK: {0:?}")]
    DiagnosticMessageNack(DiagnosticAckCode),
}
