//! # DOIP
//!
//! DOIP is a protocol for vehicle diagnostics over IP networks.
//! This library implements the networking protocol specified in [ISO 13400-2](https://www.iso.org/standard/74785.html).
//!
//!
//! ## Design
//!

pub mod messages;

/// The client feature enables
#[cfg(feature = "client")]
pub mod client;
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
