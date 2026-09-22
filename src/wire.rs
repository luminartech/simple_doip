//! Wire-codec types re-exported from [`automotive_wire_codec`].
//!
//! [`MessageError`](crate::messages::MessageError) carries [`Incomplete`],
//! [`TrailingBytes`] and [`WriteError`] in its public variants, and every message
//! type implements [`Decode`]/[`Encode`] — whose `encode` takes `&mut impl Sink`.
//! Those are foreign types, so consumers need access to them — this module
//! provides it without forcing a direct `automotive-wire-codec` dependency that
//! would have to be held in exact version lockstep. [`SliceSink`] is included
//! too: it is the sink a caller reaches for to encode into a plain `&mut [u8]`.
//!
//! # Semver
//!
//! Because these are re-exports of a foreign crate's types, `automotive-wire-codec`'s
//! semver is part of this crate's public API. A codec `0.4` is a breaking change to
//! `simple_doip` even when `simple_doip`'s own code is unchanged.

pub use automotive_wire_codec::{
    Decode, Encode, Incomplete, Sink, SliceSink, TrailingBytes, WriteError,
};
