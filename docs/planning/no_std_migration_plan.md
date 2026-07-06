# simple_doip no_std / no_alloc implementation plan

Goal: the protocol core of `simple_doip` compiles for bare-metal targets (`no_std`, no
alloc) while the tokio client/server remains available behind opt-in features for x86
tooling. Template crates: `~/dev/uds_protocol` (traits, feature forwarding) and
`~/dev/simple_someip` @ v0.8.0 (module shape, lean defaults — but do NOT copy its
Cargo.toml, which enables `embedded-io/std` unconditionally).

Settled decisions:
- RX is **zero-copy**: decode borrows from the RX buffer. No heapless, no capacity
  const-generics.
- **Lean defaults**: `default = []`. Everything std is opt-in.
- Version bumps to 0.2.0 (breaking change).

Execute work packages **in order**. Each WP ends with checkpoint commands that must pass
before moving on. Do not start a WP until the previous WP's checkpoint passes.

Two work packages additionally end with an **adversarial review gate** (see "Review
gates" at the bottom). A review gate is performed by a reviewer who did NOT write the
code (a separate agent or person). The implementer does not self-certify these.

---

## WP0 — Toolchain prep

```sh
rustup target add thumbv7em-none-eabihf
```

Checkpoint: `rustup target list --installed | grep thumbv7em-none-eabihf`

---

## WP1 — Cargo.toml

Replace the `[dependencies]`, `[dev-dependencies]`, and `[features]` sections and bump
the version. Final file content (keep `[package]` fields not shown here as-is):

```toml
[package]
name = "simple_doip"
version = "0.2.0"
edition = "2024"

[dependencies]
embedded-io = { version = "0.7", default-features = false }
strum = { version = "0.27", default-features = false, features = ["derive"] }
thiserror = { version = "2", default-features = false }
tracing = { version = "0.1", default-features = false }
# std-only, all optional
async-trait = { version = "0.1", optional = true }
bytes = { version = "1", optional = true }
futures = { version = "0.3", optional = true }
tokio = { version = "1", optional = true, features = [
    "macros",
    "rt",
    "rt-multi-thread",
    "time",
] }
tokio-util = { version = "0.7", optional = true, features = ["net", "codec"] }

[dev-dependencies]
anyhow = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tracing-subscriber = { version = "0.3", features = ["fmt"] }

[features]
default = []
alloc = ["embedded-io/alloc"]
std = ["alloc", "embedded-io/std", "thiserror/std", "tracing/std"]
codec = ["std", "dep:tokio", "dep:tokio-util", "dep:bytes"]
client = ["codec", "dep:async-trait", "dep:futures"]
server = ["codec", "dep:async-trait", "dep:futures"]

[[test]]
name = "integration_test"
required-features = ["client", "server"]

[[example]]
name = "simple_client"
required-features = ["client"]

[[example]]
name = "echo_server"
required-features = ["server"]
```

Notes:
- `byteorder` and unconditional `bytes`/`futures`/`tokio` are removed on purpose. Do not
  add `byteorder-embedded-io`; WP3 introduces slice helpers instead.
- `tracing-subscriber` moves to dev-dependencies.

Checkpoint: `cargo metadata --format-version 1 > /dev/null` (parses). The crate will NOT
compile yet; that is expected until WP3.

---

## WP2 — lib.rs

Rewrite `src/lib.rs`:

1. Add at the very top (before the doc comments end / after them, as first attributes):
   ```rust
   #![no_std]

   #[cfg(feature = "std")]
   extern crate std;
   #[cfg(feature = "alloc")]
   extern crate alloc;
   ```
2. Module gating (replace the current module list):
   ```rust
   pub mod messages;
   pub mod connection_state;
   pub mod logical_address;
   pub use logical_address::LogicalAddress;
   mod framer;                       // new in WP3
   pub use framer::try_frame;

   #[cfg(feature = "codec")]
   pub mod message_codec;
   #[cfg(any(feature = "client", feature = "server"))]
   pub mod socket_manager;           // currently ungated — gate it
   #[cfg(feature = "client")]
   pub mod client;
   #[cfg(feature = "client")]
   pub mod client_inner;
   #[cfg(feature = "client")]
   pub mod connection;
   #[cfg(any(feature = "client", feature = "server"))]
   mod error;
   #[cfg(any(feature = "client", feature = "server"))]
   pub use error::Error;
   #[cfg(feature = "server")]
   pub mod server;
   ```
3. Replace `use tokio::time;` with `use core::time::Duration;` and change every
   `time::Duration` in the timeout constants to `Duration` (e.g.
   `pub const TCP_TIMEOUT_INITIAL_INACTIVITY: Duration = Duration::from_secs(2);`).
   `Duration` lives in `core`; the constants stay in the no_std core.

Checkpoint: none standalone (compiles after WP3).

---

## WP3 — Protocol core → no_std zero-copy

This is the big one. All of `src/messages/`, plus two new files. Everything in this WP
uses only `core`, `embedded_io`, `strum`, `thiserror` — no `std::`, no `Vec`, no
`String`, no `byteorder`.

### 3.1 New file `src/messages/traits.rs`

```rust
use super::MessageError;

/// TX-side trait: encode a value into an [`embedded_io::Write`] implementor.
pub trait Encode {
    /// Number of bytes this value will write.
    fn encoded_size(&self) -> usize;

    /// Serialize into `writer`, returning the number of bytes written.
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError>;
}

/// RX-side trait: zero-copy decode from a byte slice. The decoded value may borrow
/// from `buf` and is valid only as long as `buf` lives.
pub trait Decode<'a>: Sized {
    /// Decode from `buf`, returning `(value, remaining_bytes)`.
    ///
    /// # Errors
    /// Returns an error if `buf` is too short or contains invalid data.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError>;

    /// Decode from `buf`, requiring the entire buffer to be consumed.
    ///
    /// # Errors
    /// Returns [`MessageError::TrailingBytes`] if bytes remain after decoding.
    fn decode_exact(buf: &'a [u8]) -> Result<Self, MessageError> {
        let (value, rest) = Self::decode(buf)?;
        if rest.is_empty() {
            Ok(value)
        } else {
            Err(MessageError::TrailingBytes { count: rest.len() })
        }
    }
}
```

Export from `src/messages/mod.rs`: `mod traits; pub use traits::{Decode, Encode};`

### 3.2 New file `src/messages/decode_util.rs` (pub(crate))

```rust
use super::MessageError;

pub(crate) fn take(buf: &[u8], n: usize) -> Result<(&[u8], &[u8]), MessageError> {
    if buf.len() < n {
        return Err(MessageError::InsufficientData { needed: n, available: buf.len() });
    }
    Ok(buf.split_at(n))
}

pub(crate) fn read_u8(buf: &[u8]) -> Result<(u8, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 1)?;
    Ok((bytes[0], rest))
}

pub(crate) fn read_u16_be(buf: &[u8]) -> Result<(u16, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 2)?;
    Ok((u16::from_be_bytes([bytes[0], bytes[1]]), rest))
}

pub(crate) fn read_u32_be(buf: &[u8]) -> Result<(u32, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 4)?;
    Ok((u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), rest))
}

pub(crate) fn read_array<const N: usize>(buf: &[u8]) -> Result<([u8; N], &[u8]), MessageError> {
    let (bytes, rest) = take(buf, N)?;
    let mut array = [0u8; N];
    array.copy_from_slice(bytes);
    Ok((array, rest))
}
```

Register in `src/messages/mod.rs`: `mod decode_util;`

### 3.3 Rework `src/messages/message_error.rs`

```rust
use crate::messages::{header::PayloadType, nack::NackCode};

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MessageError {
    #[error("Negative acknowledgement: {0:?}")]
    Nack(NackCode),
    #[error("Version Inverse Incorrect, Expected: {expected:X}, got: {value:X}")]
    VersionInverseIncorrect { expected: u8, value: u8 },
    #[error("Payload length in header does match expected payload type length: {value:?}, expected: {expected:?}")]
    PayloadLengthTooShort { value: usize, expected: u32 },
    #[error("Unexpected payload type found: {0:?}")]
    UnexpectedPayloadType(PayloadType),
    #[error("Insufficient data: needed {needed} bytes, {available} available")]
    InsufficientData { needed: usize, available: usize },
    #[error("Trailing bytes after decode: {count}")]
    TrailingBytes { count: usize },
    #[error("I/O error: {0:?}")]
    Io(embedded_io::ErrorKind),
}

impl MessageError {
    /// Map any embedded-io error to [`MessageError::Io`].
    pub(crate) fn io(err: impl embedded_io::Error) -> Self {
        MessageError::Io(err.kind())
    }
}

/// Required by `tokio_util::codec::Decoder` (its `Error` must be `From<std::io::Error>`).
#[cfg(feature = "std")]
impl From<std::io::Error> for MessageError {
    fn from(err: std::io::Error) -> Self {
        MessageError::Io(embedded_io::Error::kind(&err))
    }
}
```

The `#[from] std::io::Error` variant is gone; every `?` on a read in the old code becomes
an explicit helper call (see 3.5).

### 3.4 Generic data parameter for the three borrowing types

`DiagnosticMessage` and `DiagnosticMessageAck` hold variable-length data. Make them (and
everything containing them) generic over the data container instead of hard-coding
`Vec<u8>`:

```rust
pub struct DiagnosticMessage<D> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub user_data: D,
}

pub struct DiagnosticMessageAck<D> {
    pub source_address: LogicalAddress,
    pub target_address: LogicalAddress,
    pub ack_code: DiagnosticAckCode,
    pub previous_message_data: D,
}

pub enum Payload<D> { /* same variants; the two above carry <D> */ }

pub struct Message<D> {
    pub header: Header,
    pub payload: Payload<D>,
}
```

Add to `src/messages/mod.rs`:

```rust
/// Zero-copy message borrowing its data from an RX buffer.
pub type MessageRef<'a> = Message<&'a [u8]>;
#[cfg(feature = "alloc")]
pub type OwnedMessage = Message<alloc::vec::Vec<u8>>;
```

Rules:
- Decode impls exist ONLY for `D = &'a [u8]` (i.e. `impl<'a> Decode<'a> for Message<&'a [u8]>`).
- Encode impls are generic: `impl<D: AsRef<[u8]>> Encode for Message<D>` (same for the
  two payload structs and `Payload<D>`); inside, use `self.user_data.as_ref()`.
- Derive/keep `Clone, Debug, (Eq,) PartialEq` with `D: ...` bounds as needed (derives
  handle this automatically).
- Add owned conversion (in `mod.rs` or the respective files):
  ```rust
  #[cfg(feature = "alloc")]
  impl Message<&[u8]> {
      /// Copy borrowed payload data into an owned message.
      #[must_use]
      pub fn to_owned_message(&self) -> OwnedMessage { /* clone header, map payload,
          user_data.to_vec() / previous_message_data.to_vec() */ }
  }
  ```
  (Name it `to_owned_message` to avoid clashing with `ToOwned::to_owned`.)
- `impl<D: Default> Default for Message<D>` replaces the current `Default` (same body,
  `user_data: D::default()`).
- Constructors `Message::diagnostic_message(..., message: D)` and
  `Message::diagnostic_message_ack(..., previous_message_data: D)` become generic with
  `D: AsRef<[u8]>`; length math uses `message.as_ref().len()`.
- In `Message::routing_activation_request`, delete the
  `let mut payload = Vec::with_capacity(11); request.write(&mut payload).unwrap();`
  block and use `request.encoded_size() as u32` for the header length.

### 3.5 Convert every message file from `std::io` + byteorder to Encode/Decode

Mechanical transformation, file by file. Template — `alive_check_response.rs` complete
new body:

```rust
use crate::logical_address::LogicalAddress;

use super::decode_util::read_u16_be;
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// (keep existing doc comments)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AliveCheckResponse {
    pub source_address: LogicalAddress,
}

impl<'a> Decode<'a> for AliveCheckResponse {
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        Ok((AliveCheckResponse { source_address: LogicalAddress(source_address) }, rest))
    }
}

impl Encode for AliveCheckResponse {
    fn encoded_size(&self) -> usize {
        2
    }

    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        writer
            .write_all(&u16::to_be_bytes(self.source_address.into()))
            .map_err(MessageError::io)?;
        Ok(2)
    }
}
```

General rules for all files:
- Remove `use std::io::{Read, Write};` and all `byteorder` imports.
- `read_u8()?` → `decode_util::read_u8(buf)?` threading `(value, rest)`;
  `read_u16::<BigEndian>()?` → `read_u16_be`; `read_u32::<BigEndian>()?` → `read_u32_be`;
  `read_exact(&mut arr)?` → `read_array::<N>(buf)?`;
  `read_to_end(&mut vec)?` → the remainder slice itself (`let user_data = rest;`).
- `write_u8(x)?` → `writer.write_all(&[x]).map_err(MessageError::io)?`;
  `write_u16::<BigEndian>(x)?` → `writer.write_all(&u16::to_be_bytes(x)).map_err(MessageError::io)?`;
  same shape for u32; `write_all(&slice)?` → `.map_err(MessageError::io)?`.
- Replace `std::fmt` with `core::fmt` everywhere.
- `format!` is alloc-only. Three impls use it and must be rewritten with `write!`:
  - `header.rs` `Debug for ProtocolVersion`: `write!(f, "{} ({:#04X})", self, u8::from(*self))`
  - `header.rs` `Debug for PayloadType`: `write!(f, "{} ({:#04X})", self, u16::from(*self))`
  - `routing_activation_request.rs` `UpperHex for ActivationTypeCode`: `write!(f, "{:02X}", u8::from(*self))`

Per-file wire layouts (decode field order == encode field order):

| File / type | Layout (big-endian) | `encoded_size` |
|---|---|---|
| `header.rs` `Header` | u8 version, u8 inverse_version, u16 payload_type, u32 payload_length | 8 (add `pub const SIZE: usize = 8;`) |
| `nack.rs` `NackCode` | u8 | 1 |
| `alive_check_response.rs` | u16 source_address | 2 |
| `power_mode_info_response.rs` `DiagnosticPowerModeCode` | u8 | 1 |
| `entity_status_response.rs` | u8 node_type, u8 max_concurrent_tcp_sockets, u8 open_tcp_sockets, u32 max_data_size | 7 |
| `diagnostic_message.rs` `DiagnosticMessage<D>` | u16 src, u16 tgt, remainder = user_data | 4 + `user_data.as_ref().len()` |
| `diagnostic_message_ack.rs` `DiagnosticMessageAck<D>` | u16 src, u16 tgt, u8 ack_code, remainder = previous_message_data | 5 + data len |
| `routing_activation_request.rs` | u16 src, u8 activation_type, [u8;4] reserved, optional [u8;4] mfr | 7 / 11 |
| `routing_activation_response.rs` | u16 tester, u16 entity, u8 code, [u8;4] reserved_oem, optional [u8;4] oem | 9 / 13 |
| `vehicle_identification_response.rs` | [u8;17] vin, u16 addr, [u8;6] entity_id, [u8;6] group_id (None if all-0x00 or all-0xFF), u8 further_action, u8 sync_status | 33 |

Optional-tail decode rule (routing activation request & response): after the fixed
fields, if `rest.len() >= 4` decode `Some(read_array::<4>)`, else `None`. (This fixes the
existing `reserved_vehicle_manufacturer = None; // TODO` in the request.)

`Decode` for `DiagnosticMessage`/`DiagnosticMessageAck` is
`impl<'a> Decode<'a> for DiagnosticMessage<&'a [u8]>` and always returns an empty
remainder (they consume the buffer).

### 3.6 `payload.rs`

`Payload<D>` is not self-identifying, so it does NOT implement `Decode`. Replace
`Payload::read`/`Payload::write` with:

```rust
impl<'a> Payload<&'a [u8]> {
    /// Decode a payload of the given type from exactly the payload bytes of one message.
    pub fn decode(buf: &'a [u8], payload_type: PayloadType) -> Result<Self, MessageError> {
        // same match arms as the old `read`, each calling T::decode(buf)
        // and discarding the remainder (matches old stream behavior);
        // keep the existing todo!() arms as-is
    }
}

impl<D: AsRef<[u8]>> Encode for Payload<D> { /* match arms delegating to each variant;
    empty variants (AliveCheckRequest, DiagnosticMessageNack, EntityStatusRequest,
    VehicleIdentificationRequest) are size 0 / write nothing */ }
```

### 3.7 `mod.rs` `Message`

Replace `Message::read`/`Message::write` with:

```rust
impl<'a> Decode<'a> for Message<&'a [u8]> {
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (header, rest) = Header::decode(buf)?;   // validates version inverse
        let (payload_bytes, rest) = decode_util::take(rest, header.payload_length as usize)?;
        let payload = Payload::decode(payload_bytes, header.payload_type)?;
        Ok((Message { header, payload }, rest))
    }
}

impl<D: AsRef<[u8]>> Encode for Message<D> {
    fn encoded_size(&self) -> usize { Header::SIZE + self.payload.encoded_size() }
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        let written = self.header.encode(writer)?;
        Ok(written + self.payload.encode(writer)?)
    }
}
```

Update the two unit tests at the bottom of `mod.rs` to the new API
(`Message::decode(&buf)` / `MessageRef`).

### 3.8 New file `src/framer.rs` — sans-io framer

```rust
use crate::messages::{Decode, Header, MessageError, MessageRef};

/// Try to extract one complete DoIP message from the front of `buf`.
///
/// Returns `Ok(None)` if `buf` does not yet contain a complete message (read more
/// bytes and call again). On success returns the decoded message (borrowing from
/// `buf`) and the total number of bytes consumed; the caller advances its buffer by
/// that amount.
pub fn try_frame(buf: &[u8]) -> Result<Option<(MessageRef<'_>, usize)>, MessageError> {
    if buf.len() < Header::SIZE {
        return Ok(None);
    }
    let (header, _) = Header::decode(&buf[..Header::SIZE])?;
    let total = Header::SIZE + header.payload_length as usize;
    if buf.len() < total {
        return Ok(None);
    }
    let (message, _) = MessageRef::decode(&buf[..total])?;
    Ok(Some((message, total)))
}
```

Add unit tests in this file: incomplete header → `Ok(None)`; complete NACK frame (the
9-byte vector from `mod.rs` tests) → `Some` with `consumed == 9`; corrupt inverse →
`Err(VersionInverseIncorrect)`.

### 3.9 `logical_address.rs` and `connection_state.rs`

- `logical_address.rs`: replace any `std::fmt`/`std::` with `core::`. No other changes.
- `connection_state.rs`: no changes needed (`tracing::warn!` works with
  `default-features = false`). Verify no `std::` references.

### 3.10 no_std API test

Append to `src/messages/mod.rs` tests (or a new `#[cfg(test)] mod no_std_api_tests` in
`lib.rs`): encode a `Message` built with `diagnostic_message(..., &[0x10u8, 0x02][..])`
into a `&mut [u8]` stack buffer via `embedded_io::Write` (embedded-io implements `Write`
for `&mut [u8]`), then `try_frame` the buffer and assert round-trip equality — no `Vec`
anywhere in the test.

**WP3 checkpoint (must all pass):**

```sh
cargo check --no-default-features
cargo check --no-default-features --features alloc
cargo check --no-default-features --target thumbv7em-none-eabihf
cargo test --no-default-features        # core unit tests
```

**WP3 review gate: wire-format equivalence (adversarial).** See "Review gates" below —
Gate A must be passed before starting WP4.

---

## WP4 — std side: codec, client, server

Everything here is behind `codec`/`client`/`server` features.

### 4.1 `src/message_codec.rs`

Rewrite `Decoder` to wrap the framer; `Message` in the tokio layer means `OwnedMessage`:

```rust
impl Decoder for MessageCodec {
    type Item = OwnedMessage;
    type Error = MessageError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match crate::try_frame(src.as_ref()) {
            Ok(Some((message, consumed))) => {
                let owned = message.to_owned_message();
                let _ = src.split_to(consumed);
                Ok(Some(owned))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Encoder<&OwnedMessage> for MessageCodec {
    type Error = MessageError;
    fn encode(&mut self, message: &OwnedMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(message.encoded_size());
        // bytes::BufMut writer is std::io::Write; embedded-io's std feature provides
        // an adapter: wrap with embedded_io_adapters if needed, OR encode into a
        // Vec<u8> and extend dst. Simplest correct version:
        let mut out = alloc::vec::Vec::with_capacity(message.encoded_size());
        message.encode(&mut out).map_err(MessageError::from_encode)?; // Vec<u8> impls embedded_io::Write via "alloc" feature
        dst.extend_from_slice(&out);
        Ok(())
    }
}
```

Note: with `embedded-io/alloc` enabled, `Vec<u8>` implements `embedded_io::Write`
directly, so `message.encode(&mut out)` works as-is (drop the `map_err`; `encode`
already returns `MessageError`). Behavior change from today: the old code returned
`Ok(None)` on a corrupt header (stalling forever); the new code surfaces the error.
This is intentional — note it in the commit message.

### 4.2 `client.rs`, `client_inner.rs`, `connection.rs`, `server.rs`, `socket_manager.rs`

Mechanical: these currently use `Message` with owned `Vec<u8>` data. Change every
`Message` type reference to `OwnedMessage` (import from `crate::messages`). Call sites
constructing messages (`Message::diagnostic_message(...)` etc.) keep working because the
constructors are generic — pass `Vec<u8>` as before. Fix type annotations the compiler
complains about; make no behavioral changes.

### 4.3 `error.rs`

Unchanged except: confirm it still compiles (it is std-only and keeps
`tokio::io::Error`, `Elapsed`, `String`, `TryFromIntError` variants).

**WP4 checkpoint:**

```sh
cargo check --features client,server
cargo clippy --all-targets --features client,server -- -D warnings
cargo test --features client,server
```

**WP4 review gate: behavior preservation (adversarial).** See "Review gates" below —
Gate B must be passed before starting WP5.

---

## WP5 — Housekeeping

1. `tests/integration_test.rs` and both examples: no source gating needed — the
   `required-features` stanzas from WP1 handle it. Fix any compile fallout from the
   `OwnedMessage` rename.
2. README: add a "Feature flags" section documenting the matrix from WP1, that bare
   metal uses `default-features = false`, and that development uses
   `cargo test --features client,server`.
3. Delete `docs/planning/no_std_migration_plan.md` (this file) once everything below
   passes — or keep it until review, maintainer's call.

**WP5 checkpoint:**

```sh
cargo test --features client,server            # includes integration test
cargo build --examples --features client,server
```

---

## WP6 — Bare-metal example + full verification matrix

1. New `examples/bare_metal_codec.rs` (runs on host, but written no_std-style — only
   `core` APIs in the body): build a `RoutingActivationRequest` message and a
   `DiagnosticMessage` from a stack array, `encode` into `[u8; 64]` via
   `&mut buf[..]`, then feed the bytes through `try_frame` incrementally (first a
   partial slice asserting `Ok(None)`, then the full slice) and print the decoded
   result. No `[[example]]` gating needed (uses no features), but it must compile with
   `cargo build --example bare_metal_codec --no-default-features`.
2. Full matrix — all must pass:

```sh
cargo check --no-default-features
cargo check --no-default-features --features alloc
cargo check --no-default-features --features std
cargo check --features client,server
cargo check --no-default-features --target thumbv7em-none-eabihf
cargo clippy --all-targets --features client,server -- -D warnings
cargo test --no-default-features
cargo test --features client,server
cargo fmt --check
```

If the repo has CI config, add these as a job; if not, note them in the README under
"Development".

**WP6 review gate: final branch review.** Run `/code-review` (or an equivalent
independent review) on the full branch diff against `main` before merging. All
CONFIRMED correctness findings must be fixed or explicitly waived by the maintainer.

---

## Review gates (adversarial)

Rules for all gates:
- The reviewer must not be the implementer, and must be told to try to REFUTE the
  claim "this change is correct", not to summarize it.
- Every finding gets classified: CONFIRMED (reviewer demonstrated the failure with a
  concrete input or repro) or PLAUSIBLE (could not demonstrate). CONFIRMED correctness
  findings block the gate; PLAUSIBLE ones are triaged by the maintainer.
- The reviewer reports findings as `file:line`, defect statement, and a concrete
  failure scenario (input bytes → wrong output).

### Gate A — wire-format equivalence (after WP3)

Claim under attack: *every new `Decode`/`Encode` impl produces byte-for-byte identical
wire behavior to the old `read`/`write` impls* (except the two documented intentional
changes: routing-activation-request now decodes the optional manufacturer tail;
`try_frame` errors on corrupt headers instead of stalling).

Reviewer tasks:
1. For each of the 10 types in the WP3 layout table, diff the new impl against the old
   one (`git diff main -- src/messages/`) field by field: order, width, endianness,
   optional-tail condition, `encoded_size` arithmetic, sentinel handling
   (`group_id` all-0x00/all-0xFF → `None`).
2. Hunt the classic zero-copy bugs specifically: off-by-one in `take`, remainder
   returned from the wrong split half, `payload_length as usize` overflow behavior on
   32-bit targets, decode-consumes-less-than-`payload_length` silently accepting
   garbage, `decode_exact` vs. trailing-byte tolerance mismatches with old behavior.
3. Write (or demand) roundtrip property tests: for each payload type, arbitrary valid
   field values → encode → `try_frame` → equality; and old-vs-new golden vectors —
   capture encode output of at least 3 representative values per type from `main`
   (run the old code!) and assert the new code produces identical bytes.

### Gate B — behavior preservation of the std layer (after WP4)

Claim under attack: *client/server behavior is unchanged apart from the documented
corrupt-header error change*.

Reviewer tasks:
1. Audit the borrow→owned boundary in `message_codec.rs`: no use of `src` after
   `split_to`, `consumed` matches what was decoded, partial-frame path leaves `src`
   untouched.
2. Audit feature gating: `cargo check --no-default-features` from a clean tree, plus
   `grep -rn "std::\|tokio::\|Vec<\|String" src/messages/ src/framer.rs
   src/connection_state.rs src/logical_address.rs src/lib.rs` — every hit must be
   behind `#[cfg(feature = ...)]`, in a `#[cfg(test)]` block, or a doc comment.
3. Run the integration test and both examples; compare observable behavior (messages
   exchanged, log output) against `main`.
