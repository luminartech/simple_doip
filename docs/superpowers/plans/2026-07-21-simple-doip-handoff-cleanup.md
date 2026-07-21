# simple_doip Handoff Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring `simple_doip` from "L0 migration complete" to a state a new owner can take over and that could be published to crates.io.

**Architecture:** Five phases on branch `feat/l0-migration`. Phase 1 changes behavior and carries tests. Phases 2–5 are behavior-neutral, proven by 33 frozen golden wire vectors that must stay byte-identical throughout.

**Tech Stack:** Rust 2024 edition, `no_std` core with `alloc`/`std`/`codec`/`client`/`server` feature tiers, `automotive-wire-codec` 0.3 for wire primitives, `tokio` + `tokio-util` for the async layer, `thiserror`, `embedded-io`.

**Spec:** `docs/superpowers/specs/2026-07-21-simple-doip-handoff-cleanup-design.md`

## Global Constraints

- **Golden vectors are frozen.** `git diff 62a07d5..HEAD -- tests/golden/` must show no content changes at any point. In Phases 2–5 a changed vector means the change was not behavior-neutral — revert it.
- **Never run `GOLDEN_WRITE=1`.** That regenerates fixtures and destroys the invariant. If a golden test fails, the code is wrong, not the fixture.
- **Review diffs against `2e17cd3`, not `main`.** `main` is many commits behind and lacks pre-migration fixes; a `main` diff shows spurious divergence.
- **Verification after every task:**
  ```sh
  cargo test --all-features
  cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
  ```
- **Clippy runs with `-Dclippy::pedantic`.** Code that passes plain clippy may still fail. Expect to satisfy `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`.
- **`no_std` is the default build.** `default = []`. Anything using `std` types must sit behind `#[cfg(feature = "...")]`. Check with `cargo check --no-default-features`.
- Crate name is `simple_doip`. The dependency crate is `automotive-wire-codec` (hyphens), imported as `automotive_wire_codec`.
- Commit after every task. Do not batch.

---

# Phase 1 — Behavior fixes

Two tasks. These are the only changes in the effort that alter observable behavior.

---

### Task 1: Add the four missing `encoded_size` overrides

Six message types override `encoded_size` with a closed form; four do not and fall back to the codec's `CountingSink`, which runs a full extra `encode` pass to measure. The four are exactly the variable-length types, and `DiagnosticMessage` is the TX hot path — `src/message_codec.rs:45` calls `encoded_size()` on every outgoing frame.

**Files:**
- Modify: `src/messages/diagnostic_message.rs:76-89`
- Modify: `src/messages/diagnostic_message_ack.rs:161-176`
- Modify: `src/messages/routing_activation_request.rs:118-136`
- Modify: `src/messages/routing_activation_response.rs:134-151`
- Test: `tests/golden_vectors.rs:46-97`

**Interfaces:**
- Consumes: `automotive_wire_codec::Encode`, re-exported as `crate::messages::Encode`. The trait provides `fn encoded_size(&self) -> Result<usize, Self::Error>` with a `CountingSink` default.
- Produces: nothing new. Later tasks rely only on `encoded_size()` being cheap and correct.

- [ ] **Step 1: Strengthen the golden-vector harness to assert size agreement**

This is the test. `check()` already encodes every wire type across all 33 vectors, so one assertion closes the whole `encoded_size`-vs-`encode` drift class.

Replace `tests/golden_vectors.rs:46-97` (the `check` and `check_frame` functions, and the `NOTE:` comment above them) with:

```rust
fn check(name: &str, value: &impl Encode<Error = MessageError>) {
    let mut buf = [0u8; 128];
    let written = {
        let mut writer: &mut [u8] = &mut buf;
        value.encode(&mut writer).expect("encode failed")
    };
    // encoded_size() must agree with what encode() actually wrote. A mismatch means a
    // closed-form override has drifted from its encode(), which silently corrupts the
    // DoIP header's payload_length field.
    assert_eq!(
        value.encoded_size().expect("encoded_size failed"),
        written,
        "{name}: encoded_size() disagrees with encode()"
    );
    check_bytes(name, &buf[..written]);
}

/// Full frame: 8-byte header (length from `encoded_size`) + payload body.
fn check_frame(
    name: &str,
    protocol_version: ProtocolVersion,
    payload_type: PayloadType,
    payload: &impl Encode<Error = MessageError>,
) {
    let payload_len = payload.encoded_size().expect("payload encoded_size failed");
    let header = Header::new(
        protocol_version,
        payload_type,
        u32::try_from(payload_len).unwrap(),
    );
    let mut buf = [0u8; 128];
    let written = {
        let mut writer: &mut [u8] = &mut buf;
        let header_written = header.encode(&mut writer).expect("header encode failed");
        let payload_written = payload.encode(&mut writer).expect("payload encode failed");
        header_written + payload_written
    };
    assert_eq!(
        payload_len + Header::SIZE,
        written,
        "{name}: frame size disagrees with encoded_size()"
    );
    check_bytes(name, &buf[..written]);
}
```

Add `MessageError` to the import list at `tests/golden_vectors.rs:13-19`:

```rust
use simple_doip::messages::{
    ActivationTypeCode, AliveCheckResponse, DiagnosticAckCode, DiagnosticMessage,
    DiagnosticMessageAck, DiagnosticPowerModeCode, Encode, EntityStatusNodeType,
    EntityStatusResponse, FurtherActionRequired, Header, MessageError, NackCode, PayloadType,
    ProtocolVersion, RoutingActivationRequest, RoutingActivationResponse,
    RoutingActivationResponseCode, VehicleIdentificationResponse, VinGidSyncStatus,
};
```

- [ ] **Step 2: Run the golden tests — they must still PASS**

```sh
cargo test --test golden_vectors --all-features
```

Expected: **PASS**, all 10 tests. `CountingSink` produces correct sizes, just slowly — this step proves the harness change is sound before any override is added. If a vector fails here, the harness edit is wrong; fix it before continuing.

- [ ] **Step 3: Add the `DiagnosticMessage` override**

In `src/messages/diagnostic_message.rs`, inside `impl Encode for DiagnosticMessage<'_>`, add before `fn encode`:

```rust
    /// Closed form matching [`Self::encode`]: 2-byte source + 2-byte target + user data.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(4 + self.user_data.len())
    }
```

- [ ] **Step 4: Add the `DiagnosticMessageAck` override**

In `src/messages/diagnostic_message_ack.rs`, inside `impl Encode for DiagnosticMessageAck<'_>`, add before `fn encode`:

```rust
    /// Closed form matching [`Self::encode`]: 2-byte source + 2-byte target + 1-byte ack
    /// code + previous message data.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(5 + self.previous_message_data.len())
    }
```

- [ ] **Step 5: Add the `RoutingActivationRequest` override**

In `src/messages/routing_activation_request.rs`, inside `impl Encode for RoutingActivationRequest`, add before `fn encode`:

```rust
    /// Closed form matching [`Self::encode`]: 2-byte source + 1-byte activation type +
    /// 4-byte reserved, plus 4 more when the optional VM-specific tail is present.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(if self.reserved_vehicle_manufacturer.is_some() {
            11
        } else {
            7
        })
    }
```

- [ ] **Step 6: Add the `RoutingActivationResponse` override**

In `src/messages/routing_activation_response.rs`, inside `impl Encode for RoutingActivationResponse`, add before `fn encode`:

```rust
    /// Closed form matching [`Self::encode`]: 2-byte tester + 2-byte entity + 1-byte
    /// response code + 4-byte reserved OEM, plus 4 more when OEM-specific data is present.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(9 + self.oem_specific.map_or(0, |_| 4))
    }
```

- [ ] **Step 7: Run the full suite**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

Expected: all PASS. The golden vectors are the proof: if any closed form disagrees with its `encode`, Step 1's `assert_eq!` fails with `encoded_size() disagrees with encode()`.

- [ ] **Step 8: Confirm the golden fixtures did not move**

```sh
git diff --stat 62a07d5..HEAD -- tests/golden/
```

Expected: **no output** (no fixture content changed).

- [ ] **Step 9: Commit**

```bash
git add src/messages/diagnostic_message.rs src/messages/diagnostic_message_ack.rs \
  src/messages/routing_activation_request.rs src/messages/routing_activation_response.rs \
  tests/golden_vectors.rs
git commit -m "perf(messages): closed-form encoded_size for the four variable-length types

The variable-length types fell back to the codec's CountingSink default,
running a full extra encode pass to measure. DiagnosticMessage is the TX
hot path (message_codec calls encoded_size on every outgoing frame), so
this removes a redundant encode per diagnostic message.

Golden-vector harness now asserts encoded_size() == encode() for all 33
vectors, closing the drift class permanently."
```

---

### Task 2: Wire `is_framing_fatal` into the codec decode path

`MessageCodec::decode` propagates *any* `Payload::decode` error without `src.split_to(consumed)`. A recoverable body error — `UnsupportedPayloadType` being the common case — tears down the whole `FramedRead` and leaves the bad bytes in the buffer. Meanwhile `is_framing_fatal` (`src/messages/message_error.rs:52`), built in WP4 with a documented tier table, has zero production callers.

**Files:**
- Modify: `src/message_codec.rs:20-38`
- Test: `src/message_codec.rs` (new inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `MessageError::is_framing_fatal(&self) -> bool` (`src/messages/message_error.rs:52`), returning `true` only for `VersionInverseIncorrect` and `Incomplete`. `crate::try_frame(buf: &[u8]) -> Result<Option<(RawFrame<'_>, usize)>, MessageError>` (`src/framer.rs:25`).
- Produces: no signature change. `MessageCodec::decode` keeps its `Decoder` contract; only its error/skip behavior changes.

- [ ] **Step 1: Write the failing tests**

Append to `src/message_codec.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{OwnedPayload, PayloadType};

    /// Well-formed NACK frame: 8-byte header (V2012, payload type 0x0000, length 1) plus
    /// a 1-byte body.
    const NACK_FRAME: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    /// Valid header, but payload type 0x9999 is unmodeled. Framing succeeds; the body
    /// decode fails with a RECOVERABLE UnsupportedPayloadType.
    const UNSUPPORTED_FRAME: [u8; 9] = [0x02, 0xFD, 0x99, 0x99, 0x00, 0x00, 0x00, 0x01, 0x00];

    /// Corrupt inverse protocol version (0xFE, expected 0xFD): framing-FATAL.
    const CORRUPT_HEADER: [u8; 8] = [0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    #[test]
    fn partial_frame_returns_none_and_leaves_buffer_intact() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&NACK_FRAME[..4]);
        assert!(codec.decode(&mut src).unwrap().is_none());
        assert_eq!(src.len(), 4, "a partial frame must not be consumed");
    }

    #[test]
    fn two_frames_in_one_buffer_both_decode() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&NACK_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        let first = codec.decode(&mut src).unwrap().expect("first frame decodes");
        assert!(matches!(first.payload, OwnedPayload::DoIPNack(_)));
        let second = codec.decode(&mut src).unwrap().expect("second frame decodes");
        assert!(matches!(second.payload, OwnedPayload::DoIPNack(_)));
        assert!(src.is_empty(), "both frames should be consumed");
    }

    /// The regression this task exists for: a recoverable body error must skip the bad
    /// frame and keep the stream alive, not tear down the FramedRead.
    #[test]
    fn unsupported_payload_type_is_skipped_not_fatal() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&UNSUPPORTED_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        let decoded = codec
            .decode(&mut src)
            .expect("a recoverable body error must not surface as a Decoder error")
            .expect("the following valid frame should decode");
        assert!(matches!(decoded.payload, OwnedPayload::DoIPNack(_)));
        assert!(src.is_empty(), "both the skipped and the valid frame are consumed");
    }

    /// A framing-fatal error still propagates: stream sync is lost and the connection
    /// must be torn down.
    #[test]
    fn corrupt_header_is_fatal() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&CORRUPT_HEADER[..]);
        let err = codec.decode(&mut src).unwrap_err();
        assert!(err.is_framing_fatal(), "got a non-fatal error: {err:?}");
        assert!(matches!(err, MessageError::VersionInverseIncorrect { .. }));
    }

    #[test]
    fn unsupported_payload_alone_yields_none() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&UNSUPPORTED_FRAME[..]);
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "skipping the only frame leaves nothing to return"
        );
        assert!(src.is_empty(), "the skipped frame is still consumed");
        let _ = PayloadType::NegativeAcknowledge; // import is used by other tests
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test --all-features --lib message_codec
```

Expected: `unsupported_payload_type_is_skipped_not_fatal` and `unsupported_payload_alone_yields_none` **FAIL** — the current code returns `Err(UnsupportedPayloadType)` instead of skipping. The other three should pass already.

- [ ] **Step 3: Rewrite `decode` to consult the classifier**

Replace the whole `impl Decoder for MessageCodec` block in `src/message_codec.rs` with:

```rust
impl Decoder for MessageCodec {
    type Item = OwnedMessage;
    type Error = MessageError;

    /// Decode one `DoIP` message from `src`.
    ///
    /// Frames whose header is valid but whose body cannot be decoded (an unmodeled
    /// payload type, a short body) are **skipped**: the frame is consumed and decoding
    /// continues with the next one, so one unsupported message does not tear down the
    /// connection. Only framing-fatal errors — where stream sync itself is lost, per
    /// [`MessageError::is_framing_fatal`] — propagate to the caller.
    ///
    /// # Errors
    /// Returns a [`MessageError`] when framing fails fatally and the connection must be
    /// closed.
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            let Some((frame, consumed)) = crate::try_frame(src.as_ref())? else {
                return Ok(None);
            };
            let decoded = Payload::decode(frame.payload, frame.header.payload_type).map(|payload| {
                Message {
                    header: frame.header,
                    payload,
                }
                .to_owned_message()
            });
            match decoded {
                Ok(owned) => {
                    let _ = src.split_to(consumed);
                    return Ok(Some(owned));
                }
                Err(e) if e.is_framing_fatal() => return Err(e),
                Err(e) => {
                    // Recoverable: the header was sound, so `consumed` is trustworthy.
                    // Drop this frame and resync on the next one.
                    let _ = src.split_to(consumed);
                    tracing::debug!("skipping undecodable DoIP frame: {e}");
                }
            }
        }
    }
}
```

Note: `try_frame`'s own errors are framing-fatal by construction (see its doc comment at `src/framer.rs:19`), so `?` on it is correct — no classifier check needed there.

- [ ] **Step 4: Run the tests to verify they pass**

```sh
cargo test --all-features --lib message_codec
```

Expected: all 5 PASS.

If the borrow checker rejects `src.split_to(consumed)` because `frame` borrows `src`: the fix is to ensure `decoded` is fully owned before the `match` (it is — `to_owned_message()` returns `OwnedMessage`) and that `frame` is not named after the `map` call. Do **not** work around it by cloning `src`.

- [ ] **Step 5: Run the full suite**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

Expected: all PASS. Pay attention to `tests/integration_test.rs::unsupported_payload_type_does_not_kill_server` — it drives an unsupported payload type at the *server*. It should still pass; the server's connection may now stay open rather than closing, and the test tolerates both (`wait_for_connection_close` accepts EOF or reset, and the real assertion is that a fresh client can still connect).

If that test hangs, it is because the connection no longer closes. Fix the test by asserting the server survives without requiring a close — do not revert the skip behavior.

- [ ] **Step 6: Commit**

```bash
git add src/message_codec.rs
git commit -m "fix(codec): skip recoverable frames instead of killing the stream

MessageCodec::decode propagated any Payload::decode error without
consuming the frame, so a single unsupported payload type tore down the
whole FramedRead and left the bad bytes in the buffer.

Consult MessageError::is_framing_fatal, built in WP4 for exactly this
and until now called only from tests: recoverable body errors consume
the frame and resync, only fatal framing errors propagate.

Adds the first direct unit tests for message_codec."
```

---

# Phase 2 — Public API surface

Behavior-neutral. Golden vectors must not move.

---

### Task 3: Un-publish modules that export nothing usable

`client_inner` exports only `pub(super)` items; `socket_manager`'s entire surface is reachable only from `client_inner`. Both render empty or useless rustdoc pages and pin module names into the public API.

`connection_state` is *not* handled here — Task 12 deletes it outright, and un-publishing one commit before deletion is churn.

**Files:**
- Modify: `src/lib.rs:27` (`pub mod client_inner`), `src/lib.rs:35` (`pub mod socket_manager`)
- Modify: `src/socket_manager.rs:26` and its `impl` block

**Interfaces:**
- Consumes: nothing.
- Produces: `client_inner` and `socket_manager` become crate-private. No downstream task may reference them by path from outside the crate.

- [ ] **Step 1: Make the modules private**

In `src/lib.rs`, change:

```rust
#[cfg(feature = "client")]
pub mod client_inner;
```
to
```rust
#[cfg(feature = "client")]
mod client_inner;
```

and:

```rust
#[cfg(feature = "client")]
pub mod socket_manager;
```
to
```rust
#[cfg(feature = "client")]
mod socket_manager;
```

- [ ] **Step 2: Downgrade `SocketManager`'s visibility**

In `src/socket_manager.rs`, change `pub struct SocketManager<Conn>` to `pub(crate) struct SocketManager<Conn>`. Leave the method `pub`s alone for now — inside a `pub(crate)` type in a private module they are already unreachable, and `-Dclippy::pedantic` will not complain.

- [ ] **Step 3: Build and fix fallout**

```sh
cargo check --all-features
```

Expected: clean. If `unreachable_pub` or dead-code warnings appear for items that were only reachable through the old public path, that is the point — they are surfacing genuine dead code. Delete anything the compiler reports as never used, and note what you deleted in the commit message.

- [ ] **Step 4: Verify the public API actually shrank**

```sh
cargo doc --no-deps --all-features 2>&1 | tail -5
grep -rn "simple_doip::client_inner\|simple_doip::socket_manager" tests/ examples/
```

Expected: docs build succeeds; grep returns **no matches** (nothing outside `src/` used those paths).

- [ ] **Step 5: Run the full suite and commit**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
git add src/lib.rs src/socket_manager.rs
git commit -m "refactor(api): un-publish client_inner and socket_manager

Neither exported anything a consumer could use: client_inner's types are
pub(super), and SocketManager is reachable only from client_inner with no
public constructor path. Both rendered dead rustdoc pages and pinned
module names into the public API."
```

---

### Task 4: Delete dead public items

Four public items with no callers anywhere in `src/`, `tests/`, or `examples/`. One of them is also broken for external callers.

**Files:**
- Modify: `src/client.rs:18-25` (`ClientUpdate`), `src/client.rs:65-73` (`SendResult`)
- Modify: `src/connection.rs:128` and its `impl` block (`ListenerSocket`)
- Modify: `src/logical_address.rs:95-103` (`client_logical_address!`)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. Pure removal.

- [ ] **Step 1: Prove they are dead**

```sh
grep -rn "ClientUpdate\|SendResult\|ListenerSocket" src/ tests/ examples/
grep -rn "client_logical_address!" src/ tests/ examples/
```

Expected: matches **only** at the definition sites listed above. `client_logical_address` without the `!` also matches a `ClientOptions` field of the same name — that field is live and must not be touched. Only the `macro_rules!` block is dead.

If anything else matches, stop and report; do not delete a live item.

- [ ] **Step 2: Delete `ClientUpdate`**

Remove from `src/client.rs`:

```rust
#[derive(Debug, strum::Display)]
/// Send updates to the user
pub enum ClientUpdate {
    /// Unicase message from the server
    Unicast(OwnedMessage),
    /// Inner `DoIP` client error
    Error(Error),
}
```

The real update channel carries `Result<OwnedMessage, MessageError>` (`src/client.rs:85`), not this type.

- [ ] **Step 3: Delete `SendResult`**

Remove from `src/client.rs`:

```rust
/// The result of sending a message to the server. When a message
/// is suppressed (ie via UDS), the server might not respond and returns `Suppressed`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendResult<ReadDefinitions> {
    /// The message was sent successfully
    Response(ReadDefinitions),
    /// The message was sent, but the server did not respond
    Suppressed,
}
```

The generic parameter name `ReadDefinitions` is residue from a pre-`no_std` generic-client design.

- [ ] **Step 4: Delete `ListenerSocket`**

Remove `pub struct ListenerSocket;` from `src/connection.rs:128` **and** its entire `impl Connector for ListenerSocket { ... }` block that follows.

- [ ] **Step 5: Delete the broken macro**

Remove the whole `#[macro_export] macro_rules! client_logical_address { ... }` block from `src/logical_address.rs`. It is unused and its expansion emits bare `LogicalAddress($addr)` instead of `$crate::LogicalAddress($addr)`, so it only ever compiled for callers who happened to have the type in scope. Deleting is correct; fixing an unused broken macro is not.

- [ ] **Step 6: Build, test, commit**

```sh
cargo check --no-default-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

Expected: all PASS. If `Error` or `OwnedMessage` imports in `src/client.rs` are now unused, remove them.

```bash
git add src/client.rs src/connection.rs src/logical_address.rs
git commit -m "refactor(api): delete dead public items

ClientUpdate, SendResult, and ListenerSocket had no construction site
anywhere. The client_logical_address! macro was exported, unused, and
broken for external callers (bare LogicalAddress instead of \$crate::)."
```

---

### Task 5: Replace the unreachable `todo!()` with a real error, and lock the guard that makes it unreachable

See the spec's corrected §1a. The `todo!()` at `src/client.rs:174` is **not** reachable: `src/client_inner.rs:402-418` already converts a wrong-typed response into `Err(Error::UnexpectedMessageType)`. But `todo!()` reads as unfinished work and panics if a future refactor breaks the invariant.

**Files:**
- Modify: `src/client.rs:162-177`
- Test: `tests/integration_test.rs` (new test + new handler)

**Interfaces:**
- Consumes: `Error::UnexpectedMessageType(PayloadType)` (`src/error.rs:25`). `ServerConnectionHandler::routing_activation(&self, &RoutingActivationRequest) -> Result<OwnedMessage, Error>` (`src/server.rs`). `OwnedMessage::diagnostic_message_ack(ProtocolVersion, LogicalAddress, LogicalAddress, DiagnosticAckCode, Vec<u8>)` (used at `tests/integration_test.rs:110`).
- Produces: nothing new.

- [ ] **Step 1: Write the failing test**

This tests the guard in `client_inner`, which is the code that actually does the work and is currently untested. Add to `tests/integration_test.rs`:

```rust
/// A [`ServerConnectionHandler`] that answers a routing activation request with a
/// diagnostic message ack instead of a routing activation response.
struct MisbehavingHandler;

#[async_trait]
impl ServerConnectionHandler for MisbehavingHandler {
    fn get_vin(&self) -> [u8; 17] {
        [0x00; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        SERVER_LOGICAL_ADDRESS
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0x00; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, Error> {
        // Deliberately the wrong payload type for a routing activation request.
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            self.get_logical_address(),
            request.source_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            Vec::new(),
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
    ) -> Result<OwnedMessage, Error> {
        Ok(OwnedMessage::diagnostic_message_ack(
            self.protocol_version(),
            message.source_address,
            message.target_address,
            DiagnosticAckCode::RoutingConfirmationAck,
            message.user_data.to_vec(),
        ))
    }
}

/// Test 5: a server that answers routing activation with the wrong payload type must
/// produce an error, never a panic. This locks the guard at `client_inner.rs`'s
/// `is_response` check, which is what keeps `Client::connect`'s routing-activation arm
/// from ever seeing a mismatched payload.
#[tokio::test]
async fn wrong_routing_activation_response_type_errors_without_panicking() {
    let server = Server::new(MisbehavingHandler).expect("server should construct");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind test server to an ephemeral port");
    let addr = listener.local_addr().expect("bound listener has a local address");

    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let _ = server.handle_client_connection(peer_addr, stream).await;
        }
    });

    let result = with_timeout(
        "client connect against a misbehaving server",
        Client::<TestConnector>::connect(client_options(addr)),
    )
    .await;

    match result {
        Err(Error::UnexpectedMessageType(_)) => {}
        Err(other) => panic!("expected UnexpectedMessageType, got: {other:?}"),
        Ok(_) => panic!("connect should not succeed against a misbehaving server"),
    }

    accept_loop.abort();
    let _ = accept_loop.await;
}
```

- [ ] **Step 2: Run the test**

```sh
cargo test --features client,server --test integration_test wrong_routing_activation
```

Expected: **PASS** on the current code — the guard already works. This is a characterization test locking existing behavior, not a red-green cycle.

If it FAILS with a panic mentioning "Responded with something other than a routing activation response", then the `todo!()` *is* reachable and the spec's correction was itself wrong. **Stop and report** — that would make this a genuine Phase 1 bug.

If it fails by timing out or returning `Ok`, the misbehaving response is being dropped rather than routed to the oneshot. Adjust the assertion to match observed behavior and note the discrepancy in the commit message.

- [ ] **Step 3: Replace the `todo!()`**

In `src/client.rs`, change the match arm at line 163 to bind the header, and replace the `todo!()`:

```rust
                    Ok(OwnedMessage { payload, header }) => {
                        let crate::messages::OwnedPayload::RoutingActivationResponse(
                            RoutingActivationResponse {
                                logical_address_tester,
                                logical_address_of_doip_entity,
                                routing_activation_response_code,
                                reserved_oem,
                                oem_specific,
                            },
                        ) = payload
                        else {
                            // Unreachable in practice: client_inner only forwards a
                            // response whose payload type satisfies `is_response`, which
                            // for a routing activation request means exactly
                            // RoutingActivationResponse. Kept as defence in depth so a
                            // future refactor degrades to an error, not a panic.
                            return Err(Error::UnexpectedMessageType(header.payload_type));
                        };
```

- [ ] **Step 4: Verify**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

Expected: all PASS, including the new test.

- [ ] **Step 5: Commit**

```bash
git add src/client.rs tests/integration_test.rs
git commit -m "refactor(client): replace unreachable todo!() with a typed error

The arm is unreachable — client_inner's is_response check converts a
wrong-typed response to Err(UnexpectedMessageType) before it reaches
here — but todo!() reads as unfinished work and panics if a refactor
ever breaks that invariant.

Adds an integration test locking the guard that does the real work,
which was previously untested."
```

---

### Task 6: Add `pub mod wire` re-exporting the codec types

`MessageError::Incomplete` and `MessageError::TrailingBytes` (`src/messages/message_error.rs:33-36`) carry `automotive_wire_codec` types in public variants that are never re-exported. A consumer matching on them must add their own `automotive-wire-codec = "0.3"` dependency in exact version lockstep.

**Files:**
- Create: `src/wire.rs`
- Modify: `src/lib.rs`
- Modify: `src/messages/mod.rs:243`

**Interfaces:**
- Consumes: `automotive_wire_codec::{Decode, Encode, Incomplete, TrailingBytes}`.
- Produces: `simple_doip::wire::{Decode, Encode, Incomplete, TrailingBytes}` — the supported path for consumers needing the codec's types.

- [ ] **Step 1: Create the module**

Create `src/wire.rs`:

```rust
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
```

- [ ] **Step 2: Register it**

Add to `src/lib.rs`, next to the other unconditional `pub mod` declarations (this module is `no_std`-clean and needs no feature gate):

```rust
pub mod wire;
```

- [ ] **Step 3: Route the one bypass through the seam**

`src/messages/mod.rs:243` reaches for `automotive_wire_codec::take` directly — the only place bypassing the `super::traits` import seam. Add `take` to the existing import from `automotive_wire_codec` at the top of `src/messages/mod.rs` (or to `src/messages/traits.rs` if that is where the crate's other primitives are routed), then change the call site to use the imported name.

Check what the current import looks like first:

```sh
grep -n "automotive_wire_codec" src/messages/mod.rs src/messages/traits.rs
```

- [ ] **Step 4: Verify the re-export works from outside the crate**

Add to `tests/golden_vectors.rs` (a genuine external-consumer compile check):

```rust
/// Compile-time check that a consumer can name the codec's error payload types without
/// taking their own `automotive-wire-codec` dependency.
#[test]
fn wire_types_are_reachable_from_outside_the_crate() {
    fn _accepts(_: simple_doip::wire::Incomplete, _: simple_doip::wire::TrailingBytes) {}
}
```

- [ ] **Step 5: Build, test, commit**

```sh
cargo check --no-default-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

```bash
git add src/wire.rs src/lib.rs src/messages/mod.rs tests/golden_vectors.rs
git commit -m "feat(api): re-export codec types as simple_doip::wire

MessageError carries automotive_wire_codec's Incomplete and TrailingBytes
in public variants without re-exporting them, forcing consumers to take
their own version-locked codec dependency. Documents the semver
consequence: the codec's version is part of this crate's public API."
```

---

### Task 7: Turn on `missing_docs` and document the public surface

The mechanical forcing function for Phase 4. No hand-maintained list of undocumented items — let the compiler enumerate them.

**Files:**
- Modify: `src/lib.rs` (lint attributes)
- Modify: whatever the compiler reports — expect `src/error.rs`, `src/messages/message_error.rs`, `src/client.rs`, `src/server.rs`, `src/framer.rs`, `src/messages/*.rs` public fields

**Interfaces:**
- Consumes: nothing.
- Produces: a warning-free `cargo doc`, which Task 9's CI job will enforce.

- [ ] **Step 1: Turn on the lints**

Add to `src/lib.rs`, immediately after `#![no_std]`:

```rust
#![warn(missing_docs, missing_debug_implementations)]
```

- [ ] **Step 2: Enumerate the work**

```sh
cargo build --all-features 2>&1 | grep -A2 "missing documentation" | head -100
cargo build --all-features 2>&1 | grep -c "missing documentation"
```

Record the count. Known offenders from the audit, as a cross-check that the lint is catching what it should:

- `src/error.rs:9` — `Error` has no type-level doc; only 2 of 18 variants are documented; also not `#[non_exhaustive]` unlike `MessageError`
- `src/messages/message_error.rs:22` — no type-level doc; 7 variants and their named fields undocumented
- `src/client.rs:60` — `AddressType` and both variants (appears in the public `send_diagnostic_message` signature)
- `src/client.rs:81` — the `client_options` public field
- `src/server.rs:25` — `ClientConnectionInfo`; `src/server.rs:181` — `Server<T>`; the `routing_activation` and `diagnostic_message` trait methods
- `src/framer.rs:9-10` — `RawFrame::header` / `RawFrame::payload`
- `src/messages/mod.rs:46-47,55-56` — `Message` / `OwnedMessage` fields
- Public struct fields in `diagnostic_message.rs`, `diagnostic_message_ack.rs`, `entity_status_response.rs`, `routing_activation_request.rs`, `routing_activation_response.rs`, `vehicle_identification_response.rs`
- `src/logical_address.rs:18` — `OBD_ADDRESS_RANGE`

- [ ] **Step 3: Write the docs**

Work the list to zero. Rules:

- Document **what the item is in DoIP terms**, not what its Rust type is. `/// The tester's logical address, per ISO 13400-2 §7.1.` — not `/// The logical address.`
- For error variants, say **when it occurs**, not what it is named.
- Do not write `/// TODO` or restate the identifier. Those are the two failure modes here.
- Add `#[non_exhaustive]` to `Error` while you are in `src/error.rs` — it is a public error enum in a pre-1.0 crate about to be published, and `MessageError` already has it. This is a breaking change to exhaustive matches, which is correct and cheap to make now.

- [ ] **Step 4: Verify zero warnings**

```sh
cargo build --all-features 2>&1 | grep -c "missing documentation"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

Expected: count is `0`; `cargo doc` exits 0.

- [ ] **Step 5: Verify all feature combinations**

```sh
cargo build --no-default-features 2>&1 | grep -c "missing documentation"
cargo build --no-default-features --features alloc 2>&1 | grep -c "missing documentation"
cargo build --no-default-features --features std 2>&1 | grep -c "missing documentation"
```

Expected: `0` for each. Feature-gated items are easy to miss.

- [ ] **Step 6: Commit**

```bash
git add -A src/
git commit -m "docs: warn(missing_docs) and document the public API

Turns on missing_docs + missing_debug_implementations and works the
resulting list to zero, so the docs.rs page is complete before the crate
is handed off. Also marks Error #[non_exhaustive], matching MessageError."
```

---

# Phase 3 — Packaging and licensing

---

### Task 8: Lower the MSRV, add crate metadata

`Duration::from_mins` (`src/lib.rs:71`) stabilized in 1.91 and is used exactly once, for a constant whose ISO value is a fixed 300 seconds. Removing it drops the floor to whatever `edition = "2024"` requires.

**Prerequisite:** `LICENSE-MIT` and `LICENSE-APACHE` are added by the repo owner, not this task. `cargo publish --dry-run` will warn until they exist.

**Files:**
- Modify: `src/lib.rs:69-71`
- Modify: `Cargo.toml:1-4`
- Create: `rust-toolchain.toml`

**Interfaces:**
- Consumes: nothing.
- Produces: `rust-version` in `Cargo.toml`, consumed by Task 9's MSRV CI job.

- [ ] **Step 1: Replace `from_mins`**

In `src/lib.rs`, change:

```rust
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: Duration = Duration::from_mins(5);
```
to
```rust
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: Duration = Duration::from_secs(300);
```

Leave the existing doc comment unchanged — it already reads "Timeout is 300 seconds (5 minutes)", so it stays accurate.

- [ ] **Step 2: Sweep for other recently-stabilized APIs**

`from_mins` was found by inspection, not systematically. Before pinning an MSRV, confirm nothing else raises the floor:

```sh
cargo +1.85 check --all-features 2>&1 | tail -30
```

If 1.85 is not installed: `rustup toolchain install 1.85`.

If it fails, read the errors, bisect upward (`1.86`, `1.87`, …) until it builds, and use the first version that succeeds. Record the binding constraint in the commit message.

- [ ] **Step 3: Verify the floor with the full matrix**

```sh
cargo +<MSRV> check --no-default-features
cargo +<MSRV> check --no-default-features --features alloc
cargo +<MSRV> check --no-default-features --features std
cargo +<MSRV> check --features client,server
cargo +<MSRV> test --all-features
```

Expected: all PASS at the chosen `<MSRV>`.

- [ ] **Step 4: Write the metadata**

Replace the `[package]` block in `Cargo.toml`:

```toml
[package]
name = "simple_doip"
version = "0.3.0"
edition = "2024"
rust-version = "<MSRV from step 3>"
description = "An ISO 13400-2 (DoIP) implementation with a no_std, zero-copy protocol core and optional async client and server"
license = "MIT OR Apache-2.0"
repository = "https://github.com/luminartech/simple_doip"
readme = "README.md"
keywords = ["doip", "iso13400", "automotive", "diagnostics", "no-std"]
categories = ["network-programming", "embedded", "no-std"]
exclude = ["docs/", ".github/", ".vscode/"]
```

`keywords` is capped at 5 by crates.io, and each must be ≤20 characters — the list above satisfies both.

- [ ] **Step 5: Pin the toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
targets = ["thumbv7em-none-eabihf"]
```

`channel = "stable"` rather than the MSRV: the MSRV is the *floor* the crate supports, enforced by the CI job in Task 9, while day-to-day development should use current stable. Listing the embedded target here means a new owner can reproduce the `no_std` cross-check without knowing to `rustup target add`.

- [ ] **Step 6: Verify packaging**

```sh
cargo package --list --allow-dirty | wc -l
cargo package --list --allow-dirty | grep -c "^docs/"
cargo publish --dry-run --allow-dirty 2>&1 | tail -20
```

Expected: the file count drops substantially from the current 86; the `docs/` count is **0**; the dry run's only remaining complaint is the missing license *files* (which the repo owner adds separately) — not missing metadata.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/lib.rs rust-toolchain.toml
git commit -m "chore: crate metadata, MSRV floor, and toolchain pin

Duration::from_mins (1.91) was used once, for a constant whose ISO value
is a fixed 300 seconds; from_secs(300) drops the MSRV by six minor
versions at no readability cost, which matters for an embedded-targeting
crate.

Adds the description/license/repository/keywords metadata cargo publish
requires, plus exclude so the published crate stops shipping docs/."
```

---

### Task 9: Close the CI gaps

Existing CI is better than typical — it already has fmt, pedantic clippy, and a 7-permutation `no_std` matrix with an embedded cross-check. Three gaps remain.

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `rust-version` from `Cargo.toml` (Task 8).
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Add the three jobs**

Append to `.github/workflows/ci.yml`:

```yaml
  docs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo doc --no-deps --all-features
        env:
          RUSTDOCFLAGS: -D warnings

  msrv:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "<MSRV from Task 8>"
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --all-features
      - run: cargo check --no-default-features

  package:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo publish --dry-run
```

Set the `msrv` job's `toolchain` to the exact version pinned as `rust-version` in Task 8. Hardcoding it here duplicates the value, which is acceptable — the job fails loudly if the two drift, which is the desired signal.

- [ ] **Step 2: Add explicit permissions**

The workflow has no `permissions:` block, so it inherits the default token scope. For a repo about to become public, add after the `env:` block:

```yaml
permissions:
  contents: read
```

- [ ] **Step 3: Verify each job locally**

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo +<MSRV> check --all-features
cargo +<MSRV> check --no-default-features
cargo publish --dry-run --allow-dirty
```

Expected: the first three PASS. The `publish` dry run still warns about missing license files until the repo owner adds them — that is the known prerequisite from Task 8, not a failure of this task.

- [ ] **Step 4: Validate the YAML parses**

```sh
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('ok')"
```

Expected: `ok`.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add docs, MSRV, and publish dry-run jobs

Nothing ran cargo doc, which is how an empty '## Design' section and two
rotted doctests survived. Nothing pinned or defended the MSRV. Nothing
checked that the crate is publishable."
```

---

# Phase 4 — Documentation

---

### Task 10: Fill the crate-level docs

`src/lib.rs:7-8` has a `## Design` heading with an empty body. This is the docs.rs landing page.

**Files:**
- Modify: `src/lib.rs:1-8` (crate docs), `src/lib.rs:55` and `:58` (two TODO-as-doc comments)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: Replace the crate docs**

Replace `src/lib.rs:1-8` with:

```rust
//! # Simple DoIP
//!
//! An implementation of Diagnostics over IP (DoIP), the vehicle-diagnostics transport
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
//! | `alloc` | Owned mirrors ([`messages::OwnedMessage`]) that outlive the receive buffer |
//! | `std` | `std`-backed I/O and error traits |
//! | `codec` | [`message_codec::MessageCodec`], a `tokio-util` `Encoder`/`Decoder` |
//! | `client` | The async [`client::Client`] |
//! | `server` | The async [`server::Server`] |
//!
//! `default = []`, so an embedded target gets the `no_std` core with no allocator and no
//! runtime.
//!
//! ## Where to start
//!
//! - **Bare metal / sans-io:** [`try_frame`] delimits a frame from a byte buffer without
//!   owning any I/O resource; [`messages::Payload::decode`] then interprets the body.
//!   See `examples/bare_metal_codec.rs`.
//! - **Async client:** [`client::Client`] handles connection, routing activation, and
//!   acknowledgements. See `examples/simple_client.rs`.
//! - **Async server:** implement [`server::ServerConnectionHandler`] and hand it to
//!   [`server::Server`]. See `examples/echo_server.rs`.
```

Verify every intra-doc link resolves — `cargo doc` with `-D warnings` (Step 4) catches broken ones. Adjust paths if a type moved during Phase 2.

- [ ] **Step 2: Fix `TCP_TLS_PORT`'s doc**

Replace `/// TODO: Implement TLS support` at `src/lib.rs:55` with:

```rust
/// Default TCP port for `DoIP` over TLS.
///
/// Defined by ISO 13400-2 for encrypted connections. This crate does not currently
/// implement TLS; the constant is provided for callers wiring up their own TLS transport.
```

The fact that TLS is unimplemented belongs in the doc as a statement, not as a `TODO` that renders on docs.rs.

- [ ] **Step 3: Fix `TESTER_LOGICAL_ADDRESS`'s doc**

Replace `/// Is this always the address?` at `src/lib.rs:58` with:

```rust
/// A commonly used external test-equipment logical address.
///
/// ISO 13400-2 assigns the range `0x0E00..=0x0FFF` to external test equipment; this is a
/// conventional value within it, not a mandated one. Callers with an assigned tester
/// address should use that instead.
```

If the `0xE400` value is outside the `0x0E00..=0x0FFF` range the doc claims, **stop and report** — the constant and its range would be inconsistent, which is a finding, not a docs fix.

- [ ] **Step 4: Verify**

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --open
grep -rn "TODO" src/lib.rs
```

Expected: docs build clean with no broken intra-doc links; grep returns nothing.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs
git commit -m "docs: write the crate-level documentation

Fills the empty '## Design' heading (the docs.rs landing page) with the
feature tiering and a where-to-start guide, and replaces two public doc
comments that shipped a TODO and an unanswered question as their entire
documentation."
```

---

### Task 11: Fix the two broken doctests

Both doctests in `src/connection.rs` are `ignore`d and neither compiles. One predates the `no_std` migration.

**Files:**
- Modify: `src/connection.rs:1-55`

**Interfaces:**
- Consumes: `Connector::establish_connection(SocketAddr) -> Result<(OwnedReadHalf, OwnedWriteHalf), crate::Error>` (`src/connection.rs:67-69`). `Client::<Conn>::connect(ClientOptions) -> Result<Client<Conn>, Error>` — see `examples/simple_client.rs:36` for live usage.
- Produces: nothing.

- [ ] **Step 1: Replace the module docs**

Replace `src/connection.rs:1-55` with:

```rust
//! # Connection Module
//!
//! Provides the [`Connector`] trait and its default implementation [`ConnectorSocket`].
//!
//! [`Connector`] is the extension point for callers who need to control how the TCP
//! connection is established — a non-standard port, custom socket options, or a
//! pre-existing stream. [`ConnectorSocket`] connects to [`crate::TCP_PORT`] and refuses
//! any other port, so tests and non-standard deployments substitute their own.
//!
//! # Examples
//!
//! ## Default implementation
//!
//! ```no_run
//! use simple_doip::client::{Client, ClientOptions, RoutingActivationOptions};
//! use simple_doip::messages::{ActivationTypeCode, ProtocolVersion};
//! use simple_doip::LogicalAddress;
//! use std::net::{IpAddr, SocketAddr};
//!
//! # async fn example() -> Result<(), simple_doip::Error> {
//! let options = ClientOptions {
//!     server_address: SocketAddr::new("127.0.0.1".parse().unwrap(), simple_doip::TCP_PORT),
//!     server_logical_address: LogicalAddress(0x0001),
//!     server_physical_address: LogicalAddress(0x0001),
//!     client_address: IpAddr::from([0, 0, 0, 0]),
//!     client_logical_address: LogicalAddress(0x0E01),
//!     protocol_version: ProtocolVersion::V2012,
//!     routing_activation_options: Some(RoutingActivationOptions {
//!         activation_type: ActivationTypeCode::Default,
//!         oem_specific: None,
//!     }),
//! };
//! // Implicitly uses the default ConnectorSocket implementation.
//! let client = Client::connect(options).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom implementation
//!
//! ```no_run
//! use simple_doip::connection::Connector;
//! use std::net::SocketAddr;
//! use std::time::Duration;
//! use tokio::net::{TcpSocket, tcp::{OwnedReadHalf, OwnedWriteHalf}};
//!
//! pub struct MyConnector;
//!
//! #[async_trait::async_trait]
//! impl Connector for MyConnector {
//!     async fn establish_connection(
//!         gateway_address: SocketAddr,
//!     ) -> Result<(OwnedReadHalf, OwnedWriteHalf), simple_doip::Error> {
//!         let tcp_socket = TcpSocket::new_v4()?;
//!         tcp_socket.set_reuseaddr(true)?;
//!         tcp_socket.set_nodelay(true)?;
//!         let tcp_stream = tokio::time::timeout(
//!             Duration::from_millis(5100),
//!             tcp_socket.connect(gateway_address),
//!         )
//!         .await??;
//!         Ok(tcp_stream.into_split())
//!     }
//! }
//! ```
```

`no_run` compile-checks both examples without executing them, which is exactly what "needs a running server" calls for — and is what would have caught the rot.

- [ ] **Step 2: Run the doctests**

```sh
cargo test --doc --all-features
```

Expected: **2 passed; 0 ignored**. Currently it reports `0 passed; 2 ignored`.

Likely fixes if compilation fails:
- `async_trait` may not be available to doctests unless it is a dev-dependency. If the second example fails to resolve `async_trait`, add `async-trait = "0.1"` to `[dev-dependencies]` in `Cargo.toml`.
- Field names in `ClientOptions` must match `src/client.rs:41-57` exactly. Re-read that struct if a field is rejected.
- `set_nodelay` and `set_reuseaddr` return `io::Result`, and `Error` has `#[from] tokio::io::Error` (`src/error.rs:11`), so `?` converts. If not, the `From` impl may have changed in Phase 2 — check.

- [ ] **Step 3: Verify no `ignore` remains**

```sh
grep -rn "```rust,ignore\|```ignore" src/
```

Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add src/connection.rs Cargo.toml
git commit -m "docs: fix and un-ignore the connection doctests

Both were 'ignore'd and neither compiled. One referenced a crate name
'doip' that does not exist; the other showed a
Client<ProtocolResponse, ProtocolRequest> signature removed two
migrations ago, under a nonsense 'ignore(bloop)' attribute.

Converted to no_run: they compile-check without needing a live server,
which is what the stated reason for ignoring actually called for."
```

---

### Task 12: Rewrite the README

30 lines, accurate where it speaks but silent where it matters: zero lines of Rust, no mention of `automotive-wire-codec`, and a status line that contradicts handing the crate off.

**Files:**
- Modify: `README.md` (full rewrite)

**Interfaces:**
- Consumes: the feature table must match `Cargo.toml:28-34`; commands must match the `[[example]]` `required-features` at `Cargo.toml:41-47`.
- Produces: nothing.

- [ ] **Step 1: Confirm every command works before documenting it**

```sh
cargo run --example bare_metal_codec --no-default-features
cargo build --example simple_client --features client
cargo build --example echo_server --features server
cargo test --features client,server
```

Expected: all succeed. Do not document a command you have not run.

- [ ] **Step 2: Write the README**

Replace `README.md` entirely. It must contain, in this order:

1. **Title and one-sentence description** matching `Cargo.toml`'s `description`.
2. **Status** — replaces the current "intention is to open source once… much more complete". State what is implemented and what is not, drawing on the remaining `TODO`s in `src/server.rs` (no UDP vehicle announcement, single-connection-at-a-time accept loop) and `src/lib.rs` (no TLS). Do not overclaim.
3. **Quickstart** — a real, compiling Rust block adapted from `examples/bare_metal_codec.rs`, showing `try_frame` → `Payload::decode`. Copy from the working example rather than inventing.
4. **Feature flags** — keep the existing table from `README.md:11-17`; it is verified correct against `Cargo.toml`. Add a note that `alloc` is what gates the `OwnedMessage` owned mirrors, which is the practical reason to enable it.
5. **Examples** — all three, with exact commands, and an explicit note that `simple_client` and `echo_server` are a two-terminal pair (the server listens on `TCP_PORT`, the client dials `127.0.0.1:13400`):
   ```sh
   cargo run --example bare_metal_codec --no-default-features
   cargo run --example echo_server --features server    # terminal 1
   cargo run --example simple_client --features client  # terminal 2
   ```
6. **Relationship to `automotive-wire-codec`** — the wire primitives come from it, `simple_doip::wire` re-exports what consumers need, and its semver is part of this crate's API.
7. **MSRV** — the value pinned in Task 8.
8. **License** — "Licensed under either of MIT or Apache-2.0 at your option."

- [ ] **Step 3: Make the quickstart a tested doctest, not untested prose**

**Untested code in a README is exactly how the doctests in Task 11 rotted.** Do not paste a hand-written snippet.

Write the quickstart **once**, as a doctest on `try_frame` in `src/framer.rs`, so CI compiles and runs it:

```rust
/// # Examples
///
/// Frame and decode one message from a byte buffer, with no allocator:
///
/// ```
/// use simple_doip::{try_frame, messages::Payload};
///
/// // A complete DoIP NACK frame: 8-byte header + 1-byte body.
/// let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
///
/// let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
/// assert_eq!(consumed, 9);
///
/// let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
/// assert!(matches!(payload, Payload::DoIPNack(_)));
/// # Ok::<(), simple_doip::messages::MessageError>(())
/// ```
```

Run it:

```sh
cargo test --doc --all-features
```

Expected: **3 passed** (the 2 from Task 11 plus this one).

Then copy the verified block into the README's quickstart section. It is still a copy, but it is a copy of code CI proves compiles — and the README should say "see the `try_frame` docs" so the canonical version stays the tested one.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: rewrite the README for handoff

Adds what the crate is, a verified quickstart, all three examples with
working commands, the automotive-wire-codec relationship and its semver
consequence, MSRV, and license. Replaces a status line that predated the
architecture and contradicted handing the crate off."
```

---

### Task 13: Write ARCHITECTURE.md and prune the planning docs

Of ~4,100 lines under `docs/`, roughly 300 are durable. A new owner currently has seven documents about how the crate got here and zero about what it is.

**Files:**
- Create: `ARCHITECTURE.md`
- Delete: 6 files under `docs/planning/` and `docs/superpowers/plans/`, plus the untracked `docs/planning/notes/`

**Interfaces:**
- Consumes: `docs/planning/exploration-findings.md` as source material.
- Produces: `ARCHITECTURE.md`, referenced from the README.

- [ ] **Step 1: Write `ARCHITECTURE.md`**

Distil from `docs/planning/exploration-findings.md`, describing the crate **as it is now**. Cover:

- **Layering:** `no_std` borrowed core → `alloc` owned mirrors → `std` → `codec` → `client`/`server`, and why (the target profile is strict no-alloc bare metal).
- **The sans-io seam:** `try_frame` does framing only and never interprets payloads; `Payload::decode` interprets bodies. This split is what lets a caller apply its own NACK/ignore/skip policy.
- **Error taxonomy:** the two orthogonal axes documented at `src/messages/message_error.rs:1-13`, `MessageError` (`no_std` wire) vs `Error` (std transport/session), and how `is_framing_fatal` classifies recoverable-vs-fatal — now consumed by `MessageCodec::decode` as of Task 2.
- **Relationship to `automotive-wire-codec`:** what it provides, why the dependency exists, the `simple_doip::wire` re-export, and the semver coupling.
- **Known future refactors** — carry these forward so the analysis is not lost:
  - `src/messages/mod.rs` is ~687 lines across five clean seams: module facade / the two type definitions / borrowed constructors / the `Decode`+`Encode` impls / the `alloc` owned mirror / tests. A split into `builders.rs` and `owned.rs` is straightforward but was deliberately deferred — the file had just cleared two adversarial migration gates.
  - `Message::is_response` (`mod.rs:62-87`) reads only `self.header.payload_type` and never touches `self.payload`. It is a `PayloadType → PayloadType` relation and belongs on `PayloadType`; moving it would delete the `OwnedMessage::is_response` pass-through.
  - `Message::diagnostic_message_ack` (`mod.rs:142-166`) hardcodes `PayloadType::DiagnosticMessagePositiveAcknowledge` regardless of `ack_code`. **Open question, likely a bug**, deliberately deferred to a follow-up branch. There is no test coverage either way; the follow-up should add both positive and negative cases.

- [ ] **Step 2: Delete the executed planning docs**

```sh
git rm docs/planning/2026-07-15-l0-migration-plan.md \
       docs/planning/2026-07-15-l0-migration-plan-v0.3.0-reconciliation.md \
       docs/planning/2026-07-15-l0-migration-effort-design.md \
       docs/planning/no_std_migration_plan.md \
       docs/planning/l0-wire-codec-spec.md \
       docs/superpowers/plans/2026-07-15-doip-l0-planning-effort.md \
       docs/planning/exploration-findings.md
```

`exploration-findings.md` goes too — its durable content is now in `ARCHITECTURE.md`. Do not delete it before Step 1 is written and reviewed.

`l0-wire-codec-spec.md` documents the *dependency*, not this crate; it belongs in the codec repo.

- [ ] **Step 3: Delete the untracked evidence notes**

```sh
rm -rf docs/planning/notes/
```

These reference pre-migration line numbers and paths into a worktree that no longer exists. Their own plan stated they were never to be committed.

- [ ] **Step 4: Do NOT delete the spike branch**

```sh
git branch --list "spike/l0-migration"
```

Expected: the branch exists. **Leave it.** The codec sharp-edge catalog in the sibling repo cites its commit hashes (`009981a`, `05c6826`, `7082ba4`).

- [ ] **Step 5: Verify what remains**

```sh
ls -R docs/
git status --short
```

Expected: `docs/superpowers/specs/` (this effort's spec) and `docs/superpowers/plans/` (this plan) remain; `docs/planning/` is gone entirely. Nothing untracked under `docs/planning/`.

- [ ] **Step 6: Link it from the README and commit**

Add a line to `README.md` pointing at `ARCHITECTURE.md`.

```bash
git add ARCHITECTURE.md README.md
git commit -m "docs: add ARCHITECTURE.md, prune executed planning docs

A new owner had seven documents about how the crate got here and none
about what it is. Distils the durable content of exploration-findings
into an architecture doc covering the layering, the sans-io seam, the
error taxonomy, the codec relationship, and the deferred refactors.

Deletes six executed migration plans and the codec spec (which documents
a dependency, not this crate)."
```

---

# Phase 5 — Internal tidying

Behavior-neutral. Golden vectors must not move.

---

### Task 14: Collapse the six copies of the `encoded_size → u32` incantation

**Files:**
- Modify: `src/messages/mod.rs:124-129, 155-157, 186-192, 219-225, 361-367, 393-398`

**Interfaces:**
- Consumes: `Encode::encoded_size(&self) -> Result<usize, Self::Error>`.
- Produces: private `fn payload_len(value: &impl Encode<Error = MessageError>) -> u32` in `src/messages/mod.rs`. Not public; no other task depends on it.

- [ ] **Step 1: Add the helper**

Add to `src/messages/mod.rs`, near the top of the module body:

```rust
/// Payload length for a `DoIP` header, from a payload's encoded size.
///
/// # Panics
/// Panics if the payload cannot be sized, or if its encoded size exceeds `u32::MAX`.
/// Neither is reachable for a well-formed `DoIP` message: every payload type has a
/// computable size, and the wire format caps payload length at `u32::MAX` by construction.
fn payload_len(value: &impl Encode<Error = MessageError>) -> u32 {
    u32::try_from(
        value
            .encoded_size()
            .expect("DoIP message is always sizable"),
    )
    .expect("DoIP payload length exceeds u32::MAX")
}
```

- [ ] **Step 2: Replace all six call sites**

At each of the six sites, replace:

```rust
u32::try_from(x.encoded_size().expect("DoIP message is always sizable"))
    .expect("DoIP payload length exceeds u32::MAX")
```

with `payload_len(&x)`, matching the actual receiver name at each site.

```sh
grep -n "DoIP message is always sizable" src/messages/mod.rs
```

Expected after the edit: exactly **one** match — the helper's own body.

- [ ] **Step 3: Verify and commit**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
git diff --stat 62a07d5..HEAD -- tests/golden/
```

Expected: tests PASS; golden diff **empty**.

```bash
git add src/messages/mod.rs
git commit -m "refactor(messages): extract payload_len helper

Six identical copies of the encoded_size -> u32 incantation, each with
its own pair of expect messages. One helper collapses ~30 lines to 6 and
puts both panic messages in one place."
```

---

### Task 15: Delete the dead connection state machine

`src/connection_state.rs` carries `#[allow(unused)]` on both the enum and its impl. The `connection_state` field in `client_inner.rs:119` is write-only — assigned at `:153`, `:174`, `:448`, never read.

**Files:**
- Delete: `src/connection_state.rs`
- Modify: `src/lib.rs:17`, `src/client_inner.rs:6, 119, 153, 174, 448`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. Pure removal.

- [ ] **Step 1: Confirm the field is never read**

```sh
grep -n "connection_state" src/client_inner.rs
```

Expected: an import, a field declaration, and three **assignments** — no reads. If any line reads the field (uses it in a condition, match, or expression rather than as an assignment target), **stop and report**: the state machine is live and must not be deleted.

- [ ] **Step 2: Delete the module**

```sh
git rm src/connection_state.rs
```

Remove `pub mod connection_state;` from `src/lib.rs:17`.

- [ ] **Step 3: Remove the field and its assignments**

In `src/client_inner.rs`, remove:
- `connection_state::ConnectionState,` from the import block at line 6
- the `connection_state: ConnectionState,` field declaration at line 119
- `connection_state: ConnectionState::Listen,` from the struct literal at line 153
- `self.connection_state = ConnectionState::Initialized;` at line 174
- `self.connection_state = ConnectionState::Listen;` at line 448

- [ ] **Step 4: Verify and commit**

```sh
cargo check --no-default-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

Expected: all PASS.

```bash
git add -A src/ 
git commit -m "refactor(client): delete the dead connection state machine

ConnectionState carried #[allow(unused)] on both the enum and its impl,
its predicates had no callers, and client_inner's field was write-only.
A handoff reader would reasonably assume DoIP connection-state gating is
implemented; it is not. If that gating is wanted it is a feature to
design, not residue to preserve."
```

---

### Task 16: Delete the unreachable client control paths

**Files:**
- Modify: `src/client_inner.rs:18, 22, 23, 26, 196-208, 243, 251-280`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. Pure removal.

- [ ] **Step 1: Confirm the variants are never constructed**

```sh
grep -rn "ControlMessage::AliveCheckRequest\|ControlMessage::AliveCheckResponse\|ControlMessage::SendNoResponse" src/ tests/ examples/
grep -rn "send_alive_check_response" src/ tests/ examples/
```

Expected: matches only at the definition and the unreachable handler arms in `src/client_inner.rs`. If any construction site exists elsewhere, **stop and report**.

Note that client-side alive-check responses *are* sent — inline at `src/client_inner.rs:366-377`, not via `send_alive_check_response`. Do not delete that inline path.

- [ ] **Step 2: Delete the variants, their handler arms, and the dead method**

Remove from `src/client_inner.rs`:
- the `AliveCheckRequest`, `AliveCheckResponse`, and `SendNoResponse` variants of `ControlMessage`
- their match arms in `handle_control_message` (around lines 251-280)
- the `send_alive_check_response` method (lines 196-208) and its `#[allow(unused)]`
- the now-unnecessary `#[allow(unused)]` at line 18 on the `ControlMessage` enum

- [ ] **Step 3: Try removing the `too_many_lines` allow**

Remove `#[allow(clippy::too_many_lines)]` at `src/client_inner.rs:243` and check:

```sh
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

If clippy now passes, leave it removed. If it still fires, restore the attribute — the function is still long, and the goal was to see whether the deletion was enough.

- [ ] **Step 4: Verify and commit**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

```bash
git add src/client_inner.rs
git commit -m "refactor(client): delete unreachable ControlMessage variants

Three variants had no construction site anywhere, masked by an
#[allow(unused)] on the enum, along with ~50 lines of unreachable
handler. send_alive_check_response was dead too; the live alive-check
path sends inline."
```

---

### Task 17: Collapse the duplicated socket_manager error branches

`src/socket_manager.rs:178-190` and `:191-203` are copy-paste twins sharing one `concat!` overload message. The `Io` branch is unreachable on the RX path: `MessageError::Io` comes only from encode-side `embedded_io` failures (`src/messages/message_error.rs:62`), while the tokio decode path constructs `Std` via `From<std::io::Error>` (`:73`).

**Files:**
- Modify: `src/socket_manager.rs:174-208`

**Interfaces:**
- Consumes: `MessageError::{Io, Std}`.
- Produces: nothing.

- [ ] **Step 1: Read the current branches**

```sh
sed -n '170,210p' src/socket_manager.rs
```

Understand what the two arms do differently before collapsing. If they differ in more than the variant matched and the payload formatted, **do not** collapse them blindly — report the difference.

- [ ] **Step 2: Collapse to a single branch**

Both arms have the identical shape — on `ConnectionReset` log at `info` and break, otherwise log the same `concat!` overload message at `error` and break. Only the format specifier differs (`{}` vs `{:?}`). Replace lines 174-208 with:

```rust
                            Some(Err(e)) => {
                                last_activity = tokio::time::Instant::now();
                                // Socket-level errors from the tokio layer arrive as
                                // `MessageError::Std`, preserving OS error detail.
                                // `MessageError::Io` is encode-side only (embedded-io
                                // short writes) and is not expected on this RX path, but
                                // is classified identically so the match stays honest.
                                let reset = match &e {
                                    MessageError::Std(io_err) => {
                                        Some(io_err.kind() == std::io::ErrorKind::ConnectionReset)
                                    }
                                    MessageError::Io(kind) => {
                                        Some(*kind == embedded_io::ErrorKind::ConnectionReset)
                                    }
                                    _ => None,
                                };
                                if let Some(was_reset) = reset {
                                    if was_reset {
                                        info!("Connection reset by peer, closing socket: {e}");
                                    } else {
                                        error!(concat!("{}\n",
                                            "Check that you are not sending too many requests to the server.",
                                            "The server may be closing the connection due to overload."
                                        ), e);
                                    }
                                    // Either way the socket is unusable; exit the read loop.
                                    break;
                                }
                                error!("Error decoding message: {:?}", e.to_string());

                                // send a MessageError to the receiver
                                let _ = rx_tx.send(Err(e)).await;
                            }
```

This preserves both behaviors exactly: reset breaks quietly, any other socket-level error breaks loudly with the overload hint, and a decode error is forwarded to the receiver without breaking. If Step 1 showed the arms differ in any way not captured here, **stop and report** rather than applying this.

- [ ] **Step 3: Verify and commit**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
```

```bash
git add src/socket_manager.rs
git commit -m "refactor(client): collapse duplicated socket_manager error arms

The Io and Std arms were copy-paste twins with the same overload message.
Io is encode-side only and unreachable on the RX path, where the tokio
decode path always produces Std."
```

---

### Task 18: Strip migration scaffolding from the tests

**Files:**
- Modify: `src/messages/mod.rs:466-467, 483-505`
- Verify: `tests/golden_vectors.rs` (already cleaned in Task 1)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: Delete the satisfied TODO**

Remove the `// TODO: Lots more checking of payload handling` comment and the commented-out assert beneath it at `src/messages/mod.rs:466-467`. The TODO is satisfied by `alloc_conversion_tests` at `mod.rs:558`.

- [ ] **Step 2: Compare the duplicated tests before deleting**

```sh
sed -n '483,505p' src/messages/mod.rs
sed -n '1,55p' tests/nested_encode.rs
```

`test_no_std_stack_buffer_roundtrip` duplicates `tests/nested_encode.rs:10-40`. `nested_encode.rs` additionally covers the `WriteZero` tier (`:45-52`), which the inline version does not.

**Only delete the inline test if `nested_encode.rs` genuinely covers everything it asserts.** If the inline test makes an assertion `nested_encode.rs` does not, either move that assertion into `nested_encode.rs` first, or keep the inline test and skip this step. Report which you did.

- [ ] **Step 3: Delete the inline duplicate**

Remove `test_no_std_stack_buffer_roundtrip` from `src/messages/mod.rs`.

- [ ] **Step 4: Confirm coverage did not drop**

```sh
cargo test --all-features 2>&1 | grep "test result"
```

Compare the total test count against the pre-deletion count: it should drop by exactly 1 (the deleted duplicate) and no test should newly fail.

- [ ] **Step 5: Commit**

```bash
git add src/messages/mod.rs
git commit -m "test: strip migration scaffolding from the message tests

Deletes a TODO already satisfied by alloc_conversion_tests, and an inline
stack-buffer roundtrip test duplicated by tests/nested_encode.rs (which
additionally covers the WriteZero tier)."
```

---

# Final verification

Run before declaring the branch ready. Every command must pass.

- [ ] **Full test suite and lints**

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings -Dclippy::pedantic
cargo fmt -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

- [ ] **The full `no_std` feature matrix (mirrors CI)**

```sh
cargo check --no-default-features
cargo check --no-default-features --features alloc
cargo check --no-default-features --features std
cargo check --no-default-features --features server
cargo check --no-default-features --features client
cargo check --no-default-features --features codec
cargo check --features client,server
rustup target add thumbv7em-none-eabihf
cargo check --no-default-features --target thumbv7em-none-eabihf
cargo test --no-default-features
cargo run --example bare_metal_codec --no-default-features
```

- [ ] **The invariant**

```sh
git diff --stat 62a07d5..HEAD -- tests/golden/
```

Expected: **empty**. The 33 wire vectors are byte-identical to their pre-migration capture. If anything changed, find out which task did it before going further.

- [ ] **Doctests actually run**

```sh
cargo test --doc --all-features
```

Expected: `3 passed; 0 ignored` — the two fixed `connection.rs` examples from Task 11 plus the `try_frame` quickstart from Task 12. Before this effort it reported `0 passed; 2 ignored`.

- [ ] **Packaging**

```sh
cargo package --list --allow-dirty | grep -c "^docs/"
cargo publish --dry-run
```

Expected: `0` docs files. The dry run passes once the repo owner has added `LICENSE-MIT` and `LICENSE-APACHE`.

- [ ] **No residue left behind**

```sh
grep -rn "todo!\|TODO" src/ | grep -v "^src/server.rs\|^src/connection.rs:72\|^src/socket_manager.rs\|^src/messages/routing_activation_request.rs\|^src/messages/payload.rs"
```

Expected: no matches. The excluded paths hold the seven feature-gap TODOs deliberately left inline, plus `payload.rs`'s doc comments *about* previously-removed `todo!()` arms.

- [ ] **Review the whole branch**

```sh
git log --oneline 2e17cd3..HEAD
git diff --stat 2e17cd3..HEAD
```

Diff against `2e17cd3`, **never** `main`.
