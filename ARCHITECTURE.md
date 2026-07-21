# Architecture

This document describes `simple_doip` **as it is today** — the shape of the crate,
why it is shaped that way, and the known rough edges a new maintainer should be
aware of before changing anything. It is written for someone who has never seen
the code.

For usage, feature flags, and the current gap list, see [`README.md`](README.md).

> **A note on spec citations.** This repository contains no copy of ISO 13400-2.
> Nothing here cites a section, clause, or table number of the standard, and new
> documentation should not either — an earlier review of this crate found and
> removed a batch of fabricated spec locators. "per ISO 13400-2" without a
> locator is fine; a specific numbered reference that nobody can check is not.

---

## 1. What this crate is

`simple_doip` implements Diagnostics over IP (DoIP), the vehicle-diagnostics
transport specified in ISO 13400-2. A DoIP frame is an 8-byte generic header
(protocol version, its bitwise inverse, a 16-bit payload type, a 32-bit payload
length) followed by a payload whose structure is determined by the payload type.

The crate models protocol **structure** only. It knows what a diagnostic message
looks like on the wire; it does not know what the UDS bytes inside one *mean*.
That boundary is deliberate and load-bearing: the semantics of a diagnostic
identifier are application/ECU configuration, not a transport-library concern.
`Payload::DiagnosticMessage` carries `user_data: &[u8]` and nothing more.

The forcing function for the current design is a next-generation sensor that must
act as a diagnostic server/ECU on **strict no-alloc bare metal** — no allocator,
no `Vec`, no owned forms, everything borrows plus fixed buffers. Every layering
decision below follows from that.

---

## 2. Layering

Capability is added in strict Cargo-feature tiers, each building on the previous.
`default = []`, so the bare crate is the `no_std` core.

```
              (no feature)          alloc            std           codec         client / server
             ┌──────────────┐  ┌──────────────┐ ┌───────────┐ ┌─────────────┐ ┌────────────────┐
  layer      │ borrowed core│→ │ owned mirrors│→│ std I/O   │→│ tokio-util  │→│ async Client   │
             │  Message<'a> │  │ OwnedMessage │ │ + errors  │ │ MessageCodec│ │ / Server       │
             └──────────────┘  └──────────────┘ └───────────┘ └─────────────┘ └────────────────┘
   target       bare metal        bare metal          desktop / host tooling
                                  w/ allocator
```

| Tier | Cargo feature | What it adds | Key files |
|---|---|---|---|
| L0 (external) | — | byte-level `Decode`/`Encode` traits, `Incomplete`, `TrailingBytes`, `take` | `automotive-wire-codec` crate |
| Borrowed core | *(none)* | `Message<'a>`, `Payload<'a>`, `Header`, `try_frame` | `src/messages/`, `src/framer.rs` |
| Owned mirror | `alloc` | `OwnedMessage`, `OwnedPayload`, `OwnedDiagnosticMessage`, `OwnedDiagnosticMessageAck` | `src/messages/mod.rs`, `src/messages/payload.rs` |
| std | `std` | `std::io::Error` interop (`MessageError::Std`), `std`-backed error traits | `src/messages/message_error.rs` |
| Codec | `codec` | `MessageCodec`, a `tokio_util::codec` `Encoder`/`Decoder` | `src/message_codec.rs` |
| Async client | `client` | `Client`, `Connector` (trait + `ConnectorSocket`) | `src/client.rs`, `src/client_inner.rs`, `src/socket_manager.rs`, `src/connection.rs` |
| Async server | `server` | `Server`, `ServerConnectionHandler` | `src/server.rs` |

`client` and `server` are each defined in `Cargo.toml` as `["codec", ...]`, so
either one alone pulls in `codec` (and transitively `std`/`alloc`) automatically.
They do **not** pull in each other: `client_inner.rs`, `socket_manager.rs`,
`connection.rs`, and `Connector` are all gated `#[cfg(feature = "client")]`
only, so a `server`-only build gets none of them — only `server.rs`.
`src/error.rs` (the `Error` type) is gated on `client` **or** `server`, so it is
present in either build.

Verified against `Cargo.toml` `[features]` and the `#[cfg(feature = ...)]` module
gating in `src/lib.rs`.

### 2.1 Why borrowed-first

The core type is `Message<'a>`, which borrows its payload bytes directly out of
the receive buffer. Nothing in the default build allocates. An earlier design
threaded a generic data parameter (`Message<D>`) through the core so that the
same type could be either borrowed or owned; that was removed. The core is now
literally `<'a>`, and the owned form is a **separate type in a higher tier**
(`OwnedMessage`, gated on `alloc`) rather than an instantiation of the core.

The consequence is a small amount of duplication — `OwnedMessage` mirrors
`Message`'s constructors — but it keeps the bare-metal build free of any
allocator-shaped generic machinery, and it means "does an owned form exist" is a
pure `alloc`-tier decision that the embedded target never pays for.

The two directions of conversion are `Message::to_owned_message()` and
`OwnedMessage::as_ref()`. `Encode for OwnedMessage` deliberately delegates
through `as_ref()` so there is exactly **one** wire implementation
(`src/messages/mod.rs:482-496`); there is no second serializer that could drift.
The borrowed↔owned round trip for every `Payload` variant is locked by
`payload_conversion_roundtrip_all_variants` in `src/messages/mod.rs`.

---

## 3. The sans-io seam

The most important structural decision in the crate is that **framing and payload
interpretation are two separate steps**.

```rust
// step 1 — framing only. Never looks at the payload bytes.
let (frame, consumed) = try_frame(buf)?.expect("complete frame present");

// step 2 — interpretation, invoked by the caller, on the caller's terms.
let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
```

`try_frame` (`src/framer.rs:47-66`) validates the 8-byte header's protocol
version inverse, checks the declared `payload_length` only against how many
bytes are actually available in the buffer (never against what the payload
type requires — that check does not exist at this layer), delimits one frame,
and returns
`RawFrame<'a> { header, payload: &'a [u8] }` plus a consumed byte count. It owns
no I/O resource, performs no reads, and never interprets the payload type beyond
carrying it in the header. `Ok(None)` means "need more bytes" — this is the
stream-reassembly contract that lets the same function serve a TCP socket, an
`embedded-io` byte source, or a slice in a test.

Two properties fall out of this split, and both are the *point* of it:

1. **A caller can apply its own policy.** When `Payload::decode` fails, the caller
   still holds the header (so it knows the payload type), the raw payload bytes,
   and the consumed count. It can NACK, ignore, log, or skip and resync. If
   framing and decoding were fused, a malformed body would surface as an error
   with no frame boundary attached and the caller would have no way to continue.
2. **DoIP never decides for the caller.** DoIP's payload type set is closed, so an
   unrecognised payload type is genuinely invalid and is reported as a typed
   `Err(MessageError::UnsupportedPayloadType(..))` rather than being passed
   through as an opaque variant. But *error ≠ fatal*: whether to NACK, drop the
   connection, or skip the frame is a policy question the library refuses to
   answer.

`frame_then_decode_recoverability` in `src/framer.rs` pins both halves of this:
a valid frame decodes, and a frame with a valid header but an unmodeled payload
type still frames successfully with `frame.payload` and `consumed` intact.

`try_frame` guards against a hostile `payload_length` of `u32::MAX` by comparing
available bytes rather than computing `Header::SIZE + payload_len`, so it cannot
overflow on a 32-bit target (`huge_payload_length_does_not_overflow`).

`Message::decode` (`src/messages/mod.rs:283-296`) is the fused convenience path —
header + payload in one call — and exists for callers that have a complete
message in hand and do not need the seam. The framing-plus-decode path is the one
the codec and the bare-metal example use.

---

## 4. Error taxonomy

There are **two** error types, and they are not tiers of each other:

- **`MessageError`** (`src/messages/message_error.rs`) — wire-level encode/decode
  failures from the `no_std` core. Available in every build.
- **`Error`** (`src/error.rs`) — client/server transport and session failures:
  socket I/O, timeouts, routing activation refused, invalid logical address,
  unexpected message type for the current protocol state. Gated on
  `client`/`server`. It wraps `MessageError` via a `From` impl.

`MessageError`'s variants fall along **two orthogonal axes**, documented in the
module header at `src/messages/message_error.rs:1-13`:

| Variant | Framing tier | Layer |
|---|---|---|
| `VersionInverseIncorrect` | framing-fatal | wire decode |
| `Incomplete` | framing-fatal | wire decode |
| `TrailingBytes` | recoverable | wire decode |
| `PayloadLengthTooShort`, `UnexpectedPayloadType`, `UnsupportedPayloadType` | recoverable (body) | wire decode |
| `Io(embedded_io::ErrorKind)` | recoverable | encode / `embedded-io` |
| `Std(std::io::Error)` (feature `std`) | not a frame property at all | tokio/codec boundary |

`MessageError::is_framing_fatal()` collapses the first axis into a single
question: *has stream sync been lost?* If yes, the header cannot be trusted, the
declared `payload_length` is meaningless, the caller cannot reliably skip ahead,
and the only safe move is to close the connection. If no, the frame boundary is
known and the caller can NACK, ignore, or skip.

Both `Io` and `Std` exist because they carry different information. The `no_std`
core can only produce a flattened `embedded_io::ErrorKind`; the tokio layer has a
real `std::io::Error` with an OS code and message, and flattening it would throw
that away. `src/socket_manager.rs` matches on both when deciding whether a read
error means "connection reset" (`src/socket_manager.rs:159-184`).

Several variants are documented in-tree as "currently never produced by this
crate" — part of the public taxonomy but presently unreachable. On the
`MessageError` side: `PayloadLengthTooShort` and `UnexpectedPayloadType`. On the
`Error` side: `UnexpectedAckMessage`, `ValueOutOfRange`, `NackReceived`, and
`InvalidClientType`.

### 4.1 `is_framing_fatal` is consumed by the codec

`MessageCodec::decode` (`src/message_codec.rs:42-69`) is the one place the
classifier drives real behavior. Its loop is:

- `try_frame` returns `Ok(None)` → return `Ok(None)`, leave the buffer untouched.
- `try_frame` returns `Err` → propagate (all `try_frame` errors are framing-fatal).
- `Payload::decode` succeeds → consume `consumed` bytes, return the message.
- `Payload::decode` fails and `is_framing_fatal()` → propagate, **leave the buffer
  untouched** for the caller to decide.
- `Payload::decode` fails and is recoverable → consume `consumed` bytes, log at
  debug, and loop to the next frame.

So one unsupported payload type does not tear down a connection; that is what
`unsupported_payload_type_is_skipped_not_fatal` locks.

### 4.2 Open question: is `Incomplete` correctly classified?

`is_framing_fatal` treats `MessageError::Incomplete` as **fatal**. This is
defensible for the general "ran out of bytes" case, but it produces a result
worth questioning in the codec path:

`try_frame` itself never returns `Incomplete` — a buffer too short for a complete
frame yields `Ok(None)`. So by the time `Payload::decode` returns `Incomplete`
inside `MessageCodec::decode`, `try_frame` has **already proven that stream sync
was not lost**: the header parsed, its declared length was satisfied, and the
frame boundary is known. The body was simply shorter than its declared payload
type requires. Yet the classifier calls this fatal and the codec tears the
connection down, so a peer can kill a connection with a single frame carrying a
truncated body.

The taxonomy conflates two different things under one variant:

- *incomplete frame* — fewer bytes than the header says. Sync genuinely at risk.
- *incomplete body inside a complete frame* — the frame is delimited and skippable;
  only the body is short for its type. Arguably recoverable.

This behavior is deliberate as of today: it is documented on the variant and
pinned by `truncated_body_is_fatal_via_classifier` in `src/message_codec.rs`.
**Changing it is a design decision for the new owner**, not a bug fix — it would
mean either splitting `Incomplete` into two variants or making
`is_framing_fatal` context-sensitive, and it would move a tested behavior.

---

## 5. Relationship to `automotive-wire-codec`

The byte-level primitives are not defined here. They come from the
[`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec) crate
(pinned at `0.3` in `Cargo.toml`):

- `Decode<'a>` — `fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Self::Error>`
- `Encode` — `encoded_size()` plus `encode(&self, &mut impl embedded_io::Write)`
- `Incomplete`, `TrailingBytes` — the two structural leaf errors
- `take` — the leaf slice helper

Both traits carry an **associated `type Error`**, which is why `simple_doip` can
keep its own rich `MessageError` (with `PayloadType`-bearing domain variants)
instead of being forced onto a generic structural codec error. That was the whole
reason for the associated-type design: a single shared `CodecError` would have
discarded per-protocol detail.

The dependency exists so that sibling protocol crates in the same suite share one
trait surface rather than each hand-rolling `Decode`/`Encode` with slightly
different signatures.

`src/wire.rs` re-exports `Decode`, `Encode`, `Incomplete`, and `TrailingBytes` as
`simple_doip::wire`. Consumers need them — `MessageError` has public variants
carrying `Incomplete` and `TrailingBytes`, and every message type implements the
traits — and the re-export means a consumer does not have to add its own
`automotive-wire-codec` dependency and keep it in exact version lockstep.

**Semver coupling:** because those are re-exports of a foreign crate's types
appearing in this crate's public API, `automotive-wire-codec`'s semver is part of
`simple_doip`'s semver. A codec `0.4` is a breaking change to `simple_doip` even
if not one line of `simple_doip` changes. This is documented on the module
(`src/wire.rs:9-13`) and must be respected at release time.

Separately, `Payload`, `OwnedPayload`, and `MessageError` are all
`#[non_exhaustive]` (as is `Error`, noted in section 6). New variants can be
added to any of them in a semver-compatible release, so downstream `match`
expressions on these types must already carry a wildcard arm — this is a
deliberate hedge against exactly the kind of addition section 7.2 discusses
(a new payload-body variant for `0x8003`).

---

## 6. Module map

### `no_std` core

| Path | Role |
|---|---|
| `src/lib.rs` | Crate docs, feature-gated module tree, DoIP ports and timing constants |
| `src/framer.rs` | `RawFrame`, `try_frame` — the sans-io seam (the module itself is private; both types are re-exported at the crate root, so consumers write `simple_doip::try_frame` / `simple_doip::RawFrame`, not `simple_doip::framer::...`) |
| `src/messages/mod.rs` | `Message<'a>`, `OwnedMessage`, constructors, `Decode`/`Encode` impls |
| `src/messages/header.rs` | `Header` (8 bytes, `Header::SIZE`), `PayloadType`, `ProtocolVersion` |
| `src/messages/payload.rs` | `Payload<'a>` (14 variants) and `OwnedPayload`; `Payload::decode` dispatches on `PayloadType` |
| `src/messages/message_error.rs` | `MessageError` and `is_framing_fatal` |
| `src/messages/traits.rs` | One line: re-export of `Decode`, `Encode`, `take` from the codec crate |
| `src/messages/*.rs` (rest) | One file per concrete payload body (alive check, diagnostic message, routing activation, entity status, power mode, vehicle identification, NACK codes) |
| `src/logical_address.rs` | `LogicalAddress` newtype plus tester-range validation |
| `src/wire.rs` | Re-export surface for the codec crate's types |

`PayloadType` is a closed enum with `Reserved(u16)` and
`ReservedVehicleManufacturer(u16)` catch-alls, so an unknown wire value decodes
into a `Reserved` discriminant at the header level and only fails later, at
`Payload::decode`, with `UnsupportedPayloadType`. That ordering is what makes the
seam described above (section 3) usable.

### std / async layers

| Path | Role |
|---|---|
| `src/message_codec.rs` | `MessageCodec`: `Decoder<Item = OwnedMessage>` and `Encoder<&OwnedMessage>` |
| `src/error.rs` | `Error`, the transport/session error type (`#[non_exhaustive]`) |
| `src/connection.rs` | `Connector` trait and the default `ConnectorSocket` (TCP, 64 KiB socket buffers, refuses any port but 13400) |
| `src/socket_manager.rs` | Owns the spawned socket task; bridges `FramedRead`/`FramedWrite` to two mpsc channels; enforces the general inactivity timeout |
| `src/client_inner.rs` | The client state machine: a `ControlMessage` enum plus a select loop matching responses to pending requests |
| `src/client.rs` | Public `Client<Conn>` — connect, routing activation, send/receive diagnostic messages |
| `src/server.rs` | `Server<T>`, `ServerConnectionHandler`, `ClientConnectionInfo` |

The client is a channel sandwich, described in one comment at the top of
`src/client_inner.rs`:

```
User → Client → control_sender → Inner → SocketManager.sender  → TCP → Server
User ← Client ← update_receiver ← Inner ← SocketManager.receiver ← TCP ← Server
```

`Client` and `SocketManager` are generic over `Conn: Connector`, so a caller can
substitute its own transport (TLS, a test double, a non-TCP link) without
touching the protocol logic.

#### The pending-request lifecycle

`Inner` (`src/client_inner.rs`) tracks at most one in-flight request in
`active_request: Option<ControlMessage>`, where `ControlMessage` is either
`AwaitAck` (waiting on a `DiagnosticMessageAck`) or `AwaitResponse` (waiting on a
full reply, e.g. to a routing activation request or a diagnostic message sent
without suppress-positive-response). Each variant owns a oneshot `Sender` that
eventually completes the caller's `await`.

Three helpers read `active_request`: `take_await_ack`, `take_await_response`,
and the inline `match` arms in the deadline-timeout branch and the socket-error
branch of the `run` select loop. All of them share one invariant: **if the
stored `ControlMessage` does not match the variant a given call site is looking
for, it must be put back, not dropped.** `Option::take` empties the field
unconditionally; if the caller then discards the taken value under a
non-matching arm instead of restoring it, the oneshot `Sender` inside it is
dropped, its receiver observes a closed channel, and the in-flight request
dies silently — no error is ever sent, because there is no longer a `Sender` to
send one on. Downstream, a closed channel is easy to misinterpret as "the peer
doesn't support this operation" rather than "this crate ate the response",
which is exactly what happened in the routing-activation path this was found
through (see `negative_ack_during_routing_activation_does_not_drop_pending_request`
in `tests/integration_test.rs`).

`take_await_ack` and `take_await_response` both implement the invariant
correctly: on a non-matching variant, they reassign `self.active_request = other`
before returning `None`. Three sites that violated this invariant — taking
`active_request` unconditionally and dropping the mismatched case — were found
and fixed during this cleanup; three of the integration suite's regression
tests exist specifically to pin those fixes. Any new code that reads
`active_request` must preserve the same restore-on-mismatch shape, or a
future change can silently reintroduce a dropped-`Sender` bug.

### Tests and examples

- `tests/golden_vectors.rs` + `tests/golden/` — 33 frozen hex fixtures capturing
  the exact encoded bytes of representative values of every wire type. They were
  captured before the migration and are the migration's ground truth; **do not
  regenerate them**. Note they cover *bodies*, encoded via `Encode`; they do not
  independently pin header payload types (see section 7.2).
- `tests/integration_test.rs` — real client-against-real-server over loopback TCP.
- `tests/nested_encode.rs` — a permanent regression test for the encode hot path.
- `examples/bare_metal_codec.rs` — encode into a `[u8; N]`, frame, decode; builds
  and runs with `--no-default-features`. This is the executable proof that the
  core is genuinely allocator-free.
- `examples/simple_client.rs` / `examples/echo_server.rs` — a matched async pair.

---

## 7. Known issues and deferred work

Everything in this section was found by review during the handoff cleanup. It is
recorded here so the analysis does not have to be re-derived. None of it is
scheduled; all of it is a decision for the crate's next owner.

### 7.1 `ServerConnectionHandler::diagnostic_message` cannot express correct DoIP behavior

This is the one place where the API shape prevents a correct implementation.

```rust
async fn diagnostic_message(
    &self,
    message: &DiagnosticMessage<'_>,
) -> Result<OwnedMessage, Error>;
```
(`src/server.rs:105-108`)

DoIP prescribes that a DoIP entity receiving a diagnostic message first sends a
`DiagnosticMessageAck`, and then — separately and later — sends any functional
(e.g. UDS) response as its own `DiagnosticMessage`. Two messages, in order.

The trait returns a **single** `OwnedMessage`, and the dispatch site
(`src/server.rs:321-325`) maps that one value through `Some(..)` into
`handle_client_connection`, which writes exactly one message per received message
(`src/server.rs:280-286`). There is no path by which a handler can emit both.

So an implementer must choose: send the required acknowledgement, or send the
functional response. `examples/echo_server.rs` picks the acknowledgement and
smuggles the request bytes back inside the ack's `previous_message_data` field —
which is an echo demo, not protocol-correct behavior, and the example says so in
a comment (`examples/echo_server.rs:61-67`).

**Recommendation:** revisit the trait signature. Plausible shapes are returning a
collection, taking a sink/writer the handler can push to, or splitting the ack
decision (which the server could synthesize itself) from the response.

Note that `routing_activation` has the same single-`OwnedMessage` return shape,
but that is fine — routing activation genuinely is one request, one response.

### 7.2 `diagnostic_message_ack` hardcodes the positive acknowledgement payload type

Verified against `src/messages/mod.rs`.

`Message::diagnostic_message_ack` (`src/messages/mod.rs:178-202`) takes an
`ack_code: DiagnosticAckCode`, stores it correctly in the `DiagnosticMessageAck`
body — and then builds the header with a **hardcoded**
`PayloadType::DiagnosticMessagePositiveAcknowledge` (0x8002), regardless of that
code. `OwnedMessage::diagnostic_message_ack`
(`src/messages/mod.rs:452-479`) does exactly the same thing.

The result: passing a negative `DiagnosticAckCode` produces a frame whose header
announces a *positive* acknowledgement while its body carries a negative code. A
conformant peer reading the header would be misled.
`PayloadType::DiagnosticMessageNegativeAcknowledge` (0x8003) exists and is never
selected by either constructor.

This is **deliberately deferred** to a follow-up branch, not an oversight — the
behavior is documented as a "Known limitation" on both constructors and on
`Payload::DiagnosticMessageAck` (`src/messages/payload.rs:30-41`), so callers are
warned not to rely on the header matching the ack code.

**The fix is not a one-line payload-type swap — it is a modeling gap.** Verified
against `src/messages/payload.rs`: `Payload::decode` maps `0x8003`
(`PayloadType::DiagnosticMessageNegativeAcknowledge`) to `Payload::DiagnosticMessageNack`,
which is a **unit variant carrying no body at all** — its `decode` arm ignores
the payload bytes entirely rather than parsing an ack code out of them. But
`Message::diagnostic_message_ack`/`OwnedMessage::diagnostic_message_ack` build a
multi-byte `DiagnosticMessageAck` body (ack code plus `previous_message_data`)
regardless of whether the code is positive or negative. So simply stamping
`0x8003` into the header for negative codes, without also changing which
`Payload`/`OwnedPayload` variant carries the body, would produce a frame this
crate's own decoder cannot round-trip: on receipt, `Payload::decode` would
discard the ack-code bytes and hand back a bodyless `DiagnosticMessageNack`.

That loss is not hypothetical for this crate's own client. Verified against
`src/client_inner.rs:411-431`: `Inner::process_received_message` matches on
`OwnedPayload::DiagnosticMessageAck(ref ack)` to read `ack.ack_code` and, for a
negative code, complete the pending send with `Err(Error::DiagnosticMessageNack(ack.ack_code))`.
If negative acks instead arrived decoded as `OwnedPayload::DiagnosticMessageNack`
(the unit variant), this match arm would simply not fire — the message would
fall through to the generic `_ => trace!(...)` arm below it, and the specific
ack code (and the `Error::DiagnosticMessageNack` path) would be silently lost.
Fixing the header hardcode correctly therefore requires deciding, first,
what `0x8003` decodes *to* in this crate — e.g. giving `DiagnosticMessageNack`
a body carrying the ack code, or dropping the separate variant and letting both
`0x8002` and `0x8003` decode into `DiagnosticMessageAck` distinguished by
`ack_code` — and only then updating `client_inner.rs` and any other match sites
to follow. That decision is left to the next owner; it is not attempted here.

**Test coverage: partially present, but blind to the header.** Confirmed by
inspection. The golden vectors for diagnostic-message acks
(`golden_diagnostic_message_ack` in `tests/golden_vectors.rs`) encode
`DiagnosticMessageAck` *bodies* directly and never go through the constructor,
so they cannot observe the header at all. `tests/integration_test.rs`'s
`NackingRoutingHandler` (used by
`negative_ack_during_routing_activation_does_not_drop_pending_request`) *does*
exercise a genuinely negative code end-to-end — it builds
`OwnedMessage::diagnostic_message_ack(.., DiagnosticAckCode::UnknownTargetAddress, ..)`
and the test itself asserts `DiagnosticAckCode::UnknownTargetAddress.is_negative_ack()`
before sending it, so this is not accidentally a positive code in disguise.
`examples/echo_server.rs`, by contrast, only ever passes
`DiagnosticAckCode::RoutingConfirmationAck`, a positive code.

But **no test anywhere asserts an ack frame's `header.payload_type`** — that
headline claim holds. The negative-ack test drives the frame end-to-end and
checks the client's resulting `Error`, not the wire header, so the hardcode
remains invisible to the suite even though a negative code is genuinely
exercised. Incidentally — verified against `Message::is_response`
(`src/messages/mod.rs:75-100`) — that test would keep passing unmodified even if
the hardcode were fixed to stamp `0x8003`: for a pending `RoutingActivationRequest`,
`is_response` only returns `true` when the received payload type equals
`RoutingActivationResponse`, so both `0x8002` and `0x8003` fall through to
`false` and the client still surfaces `Err(Error::UnexpectedMessageType(_))`
either way. That is incidental to how `is_response` happens to be written for
this pair of payload types, not a designed check for the hardcode, and should
not be read as evidence the fix is already validated. A follow-up should add a
targeted assertion on `header.payload_type` for both a positive and a negative
ack, in addition to picking a resolution to the modeling gap above.

Changing this is a **behavior change on the wire** for negative acks. It is not
covered by the frozen golden fixtures (they pin bodies), but it should be treated
as semver-relevant.

### 7.3 `src/messages/mod.rs` should be split — the seams, concretely

The file is **759 lines** and falls along six clean, non-overlapping seams. A
split was deliberately deferred: the file had just cleared two adversarial
migration gates, and moving it immediately afterwards would have thrown away the
review confidence that had just been established. Nothing about the split is
hard; it was a sequencing choice.

Current seams, with line ranges verified against the file:

| Lines | Content | Natural home |
|---|---|---|
| 1-41 | Module facade: `mod` declarations and the `pub use` re-export block for every message submodule | stays in `mod.rs` |
| 43-70 | The two type definitions: `Message<'a>` (50-57) and `OwnedMessage` (61-70) | stays in `mod.rs` |
| 72-281 | `impl Message<'a>` — `is_response` plus the six borrowed constructors | `builders.rs` |
| 283-313 | `impl Decode<'a> for Message<'a>` and `impl Encode for Message<'_>` | could stay, or `codec.rs` |
| 315-513 | The `alloc` owned mirror: `to_owned_message`, all of `impl OwnedMessage`, `Encode for OwnedMessage`, `Default for OwnedMessage` | `owned.rs` |
| 515-759 | Tests: core tests (515-610) and `alloc_conversion_tests` (616-759) | move with their subjects |

Note the seams are not evenly sized — the borrowed constructors and the owned
mirror are ~210 and ~200 lines respectively and account for most of the file.

*(Historical note: an earlier planning document described this file as ~687 lines
across five seams with `is_response` at lines 62-87 and
`diagnostic_message_ack` at 142-166. Those numbers predate later refactors and
are stale; the ranges in the table above are current.)*

### 7.4 `is_response` belongs on `PayloadType`, not `Message`

`Message::is_response` (`src/messages/mod.rs:74-100`) reads **only**
`self.header.payload_type` and never touches `self.payload` — verified by
inspection of the whole function body. It is a pure `PayloadType → PayloadType`
relation wearing a `Message` method signature.

Moving it to `PayloadType` would:
- make the relation testable without constructing a `Message` at all;
- delete `OwnedMessage::is_response` (`src/messages/mod.rs:338-342`), which today
  exists purely as an `as_ref().is_response(..)` pass-through;
- leave the single production call site (`src/client_inner.rs:437`) a one-line
  change.

### 7.5 `SocketManager::session_id` is write-only

`src/socket_manager.rs` declares `session_id: u16` (line 33), initialises it to
`0` (line 83), and increments it on every `send` (line 97). A repo-wide grep
finds no other reference — it is never read, never sent on the wire, never
exposed. It is dead state that silently wraps at 65536 sends. Either wire it to
something real or delete it.

### 7.6 Other rough edges

These are documented in `README.md` under **Status** and are repeated here only
as a pointer: no TLS; no UDP vehicle announcement or discovery; the server's
accept loop serves one TCP connection at a time; entity status and vehicle
identification requests over TCP are silently dropped;
`ClientConnectionInfo::logical_address` is hard-coded to `0x0000` because the
server tracks no per-connection state; a failed `accept()` panics the server task.

---

## 8. Invariants to preserve when changing this crate

1. **`tests/golden/` is frozen.** The 33 hex fixtures were captured from the
   pre-migration implementation and are the only external check that the wire
   format did not drift. Never regenerate them to make a test pass; a diff there
   means the change alters the wire format and needs a deliberate decision.
2. **The default build must stay allocator-free.**
   `cargo build --no-default-features` and
   `cargo run --example bare_metal_codec --no-default-features` are the guard.
3. **`try_frame` must never interpret a payload.** The moment it does, the
   sans-io seam (section 3) collapses and callers lose the ability to apply their own policy.
4. **There is exactly one encoder.** `Encode for OwnedMessage` delegates through
   `as_ref()`. Do not add a second serialization path.
5. **`automotive-wire-codec`'s semver is this crate's semver.** Bumping it is a
   public API change here.
