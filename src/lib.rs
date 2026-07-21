//! # Simple DOIP
//!
//! DOIP is a protocol for vehicle diagnostics over IP networks.
//! This library implements the networking protocol specified in [ISO 13400-2](https://www.iso.org/standard/74785.html).
//!
//!
//! ## Design
//!

#![no_std]
#![warn(missing_docs, missing_debug_implementations)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod connection_state;
pub mod logical_address;
pub mod messages;
pub mod wire;
pub use logical_address::LogicalAddress;
mod framer;
pub use framer::{RawFrame, try_frame};

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
mod client_inner;
#[cfg(feature = "client")]
pub mod connection;
#[cfg(any(feature = "client", feature = "server"))]
mod error;
#[cfg(feature = "codec")]
pub mod message_codec;
#[cfg(feature = "client")]
mod socket_manager;
#[cfg(any(feature = "client", feature = "server"))]
pub use error::Error;
#[cfg(feature = "server")]
pub mod server;

use core::time::Duration;

/// Default TCP port for `DoIP`
/// This is the port used for unencrypted connections
/// Used for:
///  * Vehicle information services
///  * Control commands
///
pub const TCP_PORT: u16 = 13400;

/// Default UDP port for `DoIP`
/// This is the port used for discovery
pub const UDP_DISCOVERY_PORT: u16 = 13400;

/// TCP port for `DoIP` over TLS, per ISO 13400-2. Not currently used by this
/// crate: connections are established in the clear via [`TCP_PORT`]; there is no
/// TLS support yet.
pub const TCP_TLS_PORT: u16 = 3496;

/// An example logical address constant of uncertain provenance.
///
/// Despite its name, this value is used exactly once in this repository — by
/// `examples/simple_client.rs`, which assigns it to `server_logical_address`,
/// i.e. the **ECU** side rather than the tester side. No test references it.
///
/// This value is **not** mandated by ISO 13400-2 — a tester's logical address is
/// assigned per-deployment from the range
/// [`LogicalAddress::MIN_CLIENT_ADDRESS`]..=[`LogicalAddress::MAX_CLIENT_ADDRESS`]
/// (`0x0E00`-`0x0FFF`), and `0xE400` falls outside that range, so it is
/// inconsistent with the tester role its name implies. Callers should supply
/// their own deployment-specific addresses rather than relying on this constant.
pub const TESTER_LOGICAL_ADDRESS: LogicalAddress = LogicalAddress(0xE400);

// DoIP timing and communication parameters

/// Initial inactivity timeout in seconds for TCP connections directly after a `TCP_DATA` socket is established. Timeout is 2 seconds.
///
/// Must complete routing activation within this time otherwise the socket is closed by the `DoIP` entity
pub const TCP_TIMEOUT_INITIAL_INACTIVITY: Duration = Duration::from_secs(2);

/// General inactivity timeout for TCP connections. Timeout is 300 seconds (5 minutes).
///
/// If no data is sent or received for this duration, the connection is closed by the `DoIP` entity
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: Duration = Duration::from_mins(5);

/// Alive check for the maximum amount of time an entity waits for an alive check response after having
/// made an alive check request. Timeout is 5 seconds.
pub const TCP_TIMEOUT_ALIVE_CHECK: Duration = Duration::from_secs(5);

/// Time between receipt of the last byte of a `DoIP` Diagnostic Message and transmission of the ACK or NACK.
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL: Duration = Duration::from_millis(50);

/// After the timeout has elapsed, the request or response is considered to be lost and the request may be repeated
///
/// Ref: `A_DoIP_Diagnostic_Message`
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE: Duration = Duration::from_secs(2);
