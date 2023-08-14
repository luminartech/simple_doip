//! # DOIP
//!
//! DOIP is a protocol for vehicle diagnostics over IP networks.
//! This library implements the networking protocol specified in [ISO 13400-2](https://www.iso.org/standard/74785.html).
//!
//!
//! ## Design
//!

pub mod error;
pub mod messages;

/// The client feature enables
#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;
