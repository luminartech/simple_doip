# simple_doip

An ISO 13400-2 (DoIP) implementation with a `no_std`, zero-copy protocol core
and optional async client and server.

[`ARCHITECTURE.md`](ARCHITECTURE.md) describes how the crate is put together —
the feature-gated layering, the sans-io framing/decode seam, the error taxonomy,
the relationship to `automotive-wire-codec`, and the known issues and deferred
refactors a new maintainer should read before changing anything.

## Scope and limitations

The protocol core — framing, message encode/decode — is golden-vector tested
against the on-wire format. The async `client` and `server` cover the common
case, within these bounds:

- **No TLS.** Connections are in the clear on `TCP_PORT` (`13400`);
  `TCP_TLS_PORT` (`3496`) is defined by ISO 13400-2 and unused here.
- **The server serves one TCP connection at a time.** A second client cannot
  connect while the first is being served.
- **No unsolicited UDP vehicle announcement.** A tester learns of an entity only
  by asking, and identification is answered on the UDP path only, by
  `Server::run_udp_responder` on a socket the caller binds and drives.
- **The client requires the peer to acknowledge before it responds.** A
  `DiagnosticMessage` that arrives while the client is waiting for the
  acknowledgement is dropped, so a peer that answers first appears never to
  answer at all.
- **`ClientConnectionInfo::logical_address` is not yet tracked per connection.**

[`ARCHITECTURE.md`](ARCHITECTURE.md) §7 has the mechanics behind each of these,
and the deferred work around them.

None of this constrains bare-metal or single-client use.

## Quickstart

The protocol core needs no allocator and no I/O: frame a byte buffer with
`try_frame`, then decode the payload with `Payload::decode`. This block is the
doctest on `try_frame` in [`src/framer.rs`](src/framer.rs), wrapped in a
`fn main` returning `Result` so it compiles as pasted (the doctest gets the
same `Ok(())` from a hidden line instead). That doctest is the canonical,
CI-tested version; if the two ever drift, trust the doctest.

```rust
use simple_doip::{try_frame, messages::{MessageError, Payload}};

fn main() -> Result<(), MessageError> {
    // A complete DoIP NACK frame: 8-byte header + 1-byte body.
    let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
    assert_eq!(consumed, 9);

    let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
    assert!(matches!(payload, Payload::DoIPNack(_)));
    Ok(())
}
```

## Feature flags

The protocol core is `no_std` and zero-copy by default. Everything that pulls in
`alloc`, `std`, or an async runtime is opt-in via Cargo features:

| Feature  | Enables                                                   | Depends on                        |
|----------|------------------------------------------------------------|------------------------------------|
| `alloc`  | Allocator-backed helpers                                    | —                                   |
| `std`    | `std`-backed I/O and error traits                            | `alloc`                            |
| `codec`  | The tokio-util `Encoder`/`Decoder` for DoIP frames           | `std`, `tokio`, `tokio-util`, `bytes` |
| `client` | The async DoIP client                                        | `codec`, `async-trait`, `futures`  |
| `server` | The async DoIP server                                        | `codec`, `async-trait`, `futures`  |

`default = []`, so bare-metal / embedded targets should build with
`default-features = false` to keep the crate `no_std` with no allocator or
runtime dependencies.

`alloc` is what gates `messages::OwnedMessage` and its owned mirrors of the
borrowed message types — the practical reason to enable it is that you need a
message to outlive the buffer it was decoded from (e.g. to move it across a
queue or task boundary).

For development and testing, enable `client` and `server`:

```sh
cargo test --features client,server
```

## Examples

- **`bare_metal_codec`** — encode, frame, and decode with no allocator and no
  I/O. Runs standalone:

  ```sh
  cargo run --example bare_metal_codec --no-default-features
  ```

- **`echo_server`** and **`simple_client`** — a matched pair, not standalone.
  The server listens on `TCP_PORT` (`13400`); the client dials
  `127.0.0.1:13400`. Run each in its own terminal, server first:

  ```sh
  cargo run --example echo_server --features server    # terminal 1
  cargo run --example simple_client --features client   # terminal 2
  ```

  The client's `ConnectorSocket` refuses any `server_address` whose port is not
  `TCP_PORT` (`13400`), returning `Error::InvalidPort` — pointing a client at a
  non-standard port requires your own `Connector` implementation.

  `echo_server` answers a diagnostic message the way DoIP prescribes: first a
  positive acknowledgement carrying the received bytes back in its
  previous-message-data field, then the echo itself as a separate
  `DiagnosticMessage`. Both are written into the `ResponseWriter` the handler
  is given, which is the shape a real UDS response takes.

  `echo_server` calls `run_server`, so it serves TCP only and does not answer
  UDP discovery probes; see `Server::run_udp_responder` for that half.

## Relationship to `automotive-wire-codec`

The wire-level primitives — byte-level `Decode`/`Encode`, `Incomplete`,
`TrailingBytes` — come from the [`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec)
crate. `simple_doip::wire` re-exports what consumers need so that using this
crate does not require a direct dependency on `automotive-wire-codec`.
Because those re-exported types appear in this crate's public API (e.g. in
`MessageError`'s variants and in every message type's trait impls),
`automotive-wire-codec`'s semver is effectively part of `simple_doip`'s own
semver: a breaking change in that crate is a breaking change here too.

## MSRV

The minimum supported Rust version is **1.88**, bound by let-chain syntax
used in this crate.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](CONTRIBUTING.md). Security reports go through GitHub's
private vulnerability reporting; see [`SECURITY.md`](SECURITY.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
