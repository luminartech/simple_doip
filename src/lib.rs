//! # Simple `DoIP`
//!
//! An implementation of Diagnostics over IP (`DoIP`), the vehicle-diagnostics transport
//! specified in [ISO 13400-2](https://www.iso.org/standard/74785.html).
//!
//! ## Design
//!
//! The protocol core is `no_std` and zero-copy: [`messages::Message`] borrows directly
//! from the receive buffer and never allocates. Wire primitives come from
//! [`automotive_wire_codec`], re-exported as [`wire`] so consumers do not need their own
//! dependency on it.
//!
//! Capability is layered by Cargo feature, each building on the previous:
//!
//! | Feature | Adds |
//! |---|---|
//! | *(none)* | `no_std` borrowed messages, [`try_frame`] framing, encode/decode |
//! | `alloc` | Owned mirrors (`messages::OwnedMessage`) that outlive the receive buffer |
//! | `std` | `std`-backed I/O and error traits |
//! | `codec` | `message_codec::MessageCodec`, a `tokio-util` `Encoder`/`Decoder` |
//! | `client` | The async `client::Client` |
//! | `server` | The async `server::Server` |
//!
//! `default = []`, so an embedded target gets the `no_std` core with no allocator and no
//! runtime.
//!
//! ## Where to start
//!
//! - **Bare metal / sans-io:** [`try_frame`] delimits a frame from a byte buffer without
//!   owning any I/O resource; [`messages::Payload::decode`] then interprets the body.
//!   See `examples/bare_metal_codec.rs`.
//! - **Bare-metal entity (server):** [`bare_metal_entity::Entity`] is a complete sans-io
//!   ISO 13400-2 entity — vehicle announcement, routing activation, diagnostic-message
//!   dispatch — driven through platform callbacks, for `no_std` targets with a single
//!   diagnostic TCP socket.
//! - **Async client:** `client::Client` handles connection, routing activation, and
//!   acknowledgements (requires the `client` feature). See `examples/simple_client.rs`.
//! - **Async server:** implement `server::ServerConnectionHandler` and hand it to
//!   `server::Server` (requires the `server` feature). See `examples/echo_server.rs`.

#![no_std]
#![warn(missing_docs, missing_debug_implementations)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod bare_metal_entity;
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
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: Duration = Duration::from_secs(300);

/// Alive check for the maximum amount of time an entity waits for an alive check response after having
/// made an alive check request. Timeout is 5 seconds.
pub const TCP_TIMEOUT_ALIVE_CHECK: Duration = Duration::from_secs(5);

/// Time between receipt of the last byte of a `DoIP` Diagnostic Message and transmission of the ACK or NACK.
///
/// This is a performance requirement on the **entity emitting the ACK**, not a
/// deadline for a tester waiting on one. Do not use it to time out a send:
/// it allows nothing for network transit or for an entity that ACKs after
/// running its handler, and an entity that is merely slow is not an entity
/// that failed. [`TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE`] is the parameter that
/// governs when a message may be considered lost.
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL: Duration = Duration::from_millis(50);

/// After the timeout has elapsed, the request or response is considered to be lost and the request may be repeated
///
/// Ref: `A_DoIP_Diagnostic_Message`
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE: Duration = Duration::from_secs(2);
