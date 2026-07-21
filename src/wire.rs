//! Wire-codec types re-exported from [`automotive_wire_codec`].
//!
//! [`MessageError`](crate::messages::MessageError) carries [`Incomplete`] and
//! [`TrailingBytes`] in its public variants, and every message type implements
//! [`Decode`]/[`Encode`]. Those are foreign types, so consumers need access to them —
//! this module provides it without forcing a direct `automotive-wire-codec` dependency
//! that would have to be held in exact version lockstep.
//!
//! # Semver
//!
//! Because these are re-exports of a foreign crate's types, `automotive-wire-codec`'s
//! semver is part of this crate's public API. A codec `0.4` is a breaking change to
//! `simple_doip` even when `simple_doip`'s own code is unchanged.

pub use automotive_wire_codec::{Decode, Encode, Incomplete, TrailingBytes};
