# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

This file was reconstructed from the commit and pull-request history when the
crate was prepared for publication, so entries before that point describe what
changed rather than what was announced at the time.

## [Unreleased]

### Added

- `MessageError::PayloadTooLarge`, for a payload that cannot be described by
  the `u32` `payload_length` field. `MessageError` is `#[non_exhaustive]`, so
  this is not a breaking change.
- `LICENSE-MIT` and `LICENSE-APACHE`. The manifest had declared
  `MIT OR Apache-2.0` without carrying either text.
- `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`.
- `release-plz.toml`. Versioning, the changelog, tags, GitHub releases
  and the crates.io publish are handled by release-plz through the
  org-wide reusable workflow, matching `uds_protocol` and
  `automotive_wire_codec`. There is no release workflow in this repo.

### Fixed

- `Message::encode` derives the header's `payload_length` from the payload
  instead of writing the field verbatim. A decoded message keeps whatever
  length the wire declared, and `Payload::decode` need not consume all of it,
  so re-encoding a frame that arrived with a mismatched length emitted a frame
  no decoder would accept. Anything that decodes and re-emits -- a proxy, a
  replay tool, a logging fake -- was turning a malformed-but-accepted frame
  into a corrupt one. Consequence to know about: for such a frame,
  `decode(encode(m)).header.payload_length` is now the payload's real size
  rather than the length it arrived with.
- `MessageError::PayloadLengthTooShort`'s message said "does match" where it
  meant "does not match".
- `ClientConnectionInfo::logical_address` carries the address the tester
  activated routing with, instead of always being `0x0000`. A handler can now
  tell which tester is asking, and the default `alive_check` answers with the
  right source address. It stays `0x0000` before activation and after an
  activation the handler denied.

### Security

- The committed lockfile pinned three versions with RUSTSEC advisories against
  them — `bytes 1.4.0`, `mio 0.8.8` and `tracing-subscriber 0.3.19`. All three
  are refreshed past their patched versions. No manifest requirement changed;
  every one was already permitted.

### Changed

- docs.rs now builds with all features, so the `client`, `server` and `codec`
  API appears in the published documentation. `default = []`, so the default
  build documents only the `no_std` core.
- `release-plz.toml` and `rust-toolchain.toml` are no longer packaged into
  the published crate.

## [0.5.2] — 2026-09-03

### Fixed

- A failed connection reported `Error::SocketNotBound` instead of the
  underlying connect error, so the reason a connection failed was discarded
  and no log level revealed it.

## [0.5.1] — 2026-08-25

### Fixed

- A response already buffered by the client was dropped when the connection
  closed, instead of being delivered to the caller waiting for it.

## [0.5.0] — 2026-08-25

### Changed

- **Breaking:** the diagnostic-message response timeout is configurable on
  `ClientOptions`, and the client now waits `A_DoIP_Diagnostic_Message` for an
  acknowledgement rather than the 50 ms figure ISO 13400-2 places on a DoIP
  *entity*. The old value timed out on any peer that took longer than 50 ms to
  acknowledge.

## [0.4.0] — 2026-08-19

### Changed

- **Breaking:** `ServerConnectionHandler::diagnostic_message` takes
  `&mut dyn ResponseWriter` and returns `Result<(), Error>`. It previously
  returned a single `OwnedMessage`, so a handler could emit either the
  `DiagnosticMessageAck` or the functional response but not both — which is
  what DoIP prescribes, and what a UDS tester waits for.

### Added

- `ResponseWriter`, so a handler can emit an acknowledgement, any number of
  "response pending" messages, and a final answer, awaiting work between them.
- `Server::run_server_with_listener`, for injecting a bound `TcpListener`.
- `Server::run_udp_responder`, answering vehicle-identification requests over
  UDP.

## [0.3.1] — 2026-08-12

### Changed

- A routing-activation denial is returned to the caller as an error instead of
  only being logged.

## [0.3.0] — 2026-07-30

### Changed

- `MessageError` is built on the `automotive-wire-codec` error fragments, which
  makes that crate's error taxonomy part of this crate's public API.

## [0.2.0] — 2026-07-30

### Changed

- **Breaking:** the protocol core is `no_std` and zero-copy. Messages borrow
  from the receive buffer instead of allocating, `alloc` gates the owned
  mirrors, and `std` and the async layers became opt-in Cargo features
  (`default = []`).

## [0.1.0]

Initial implementation: DoIP message types, framing, and an async client and
server over tokio.

<!-- Only v0.1.0 and v0.5.1 were ever tagged, so the intermediate versions
     have no comparison range to link. -->

[Unreleased]: https://github.com/luminartech/simple_doip/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/luminartech/simple_doip/compare/v0.1.0...v0.5.1
[0.1.0]: https://github.com/luminartech/simple_doip/releases/tag/v0.1.0
