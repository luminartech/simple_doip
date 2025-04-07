//! # DOIP
//!
//! DOIP is a protocol for vehicle diagnostics over IP networks.
//! This library implements the networking protocol specified in [ISO 13400-2](https://www.iso.org/standard/74785.html).
//!
//!
//! ## Design
//!

pub mod traits;

pub mod messages;

#[cfg(feature = "client")]
pub mod client;
/// The client feature enables
#[cfg(feature = "client")]
pub mod client_inner;
#[cfg(any(feature = "client", feature = "server"))]
mod error;
#[cfg(any(feature = "client", feature = "server"))]
pub use error::Error;
#[cfg(feature = "codec")]
pub mod message_codec;

#[cfg(feature = "server")]
pub mod server;

/// Default TCP port for DoIP
/// This is the port used for unencrypted connections
pub const TCP_PORT: u16 = 13400;

/// TODO: Implement TLS support
pub const TCP_TLS_PORT: u16 = 3496;

pub const LIDAR_LOGICAL_ADDRESS: u16 = 0xE400;

// DoIP timing and communication parameters

/// Initial inactivity timeout in seconds for TCP connections directly after a TCP_DATA socket is established. Timeout is 2 seconds.
///
/// Must complete routing activation within this time otherwise the socket is closed by the DoIP entity
pub const TCP_TIMEOUT_INITIAL_INACTIVITY: u32 = 2;

/// General inactivity timeout for TCP connections. Timeout is 300 seconds (5 minutes).
///
/// If no data is sent or received for this duration, the connection is closed by the DoIP entity
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: u32 = 300; // seconds (5 minutes)

/// Alive check for the maximum amount of time an entity waits for an alive check response after having
/// made an alive check request. Timeout is 5 seconds.
pub const TCP_TIMEOUT_ALIVE_CHECK: u32 = 5;
