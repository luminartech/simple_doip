# simple_doip

An ISO 13400-2 (DoIP) implementation with a `no_std`, zero-copy protocol core
and optional async client and server.

## Status

The protocol core (framing, message encode/decode, golden-vector-tested
against the on-wire format) is solid and exercised by the test suite. The
async `client` and `server` layers work for the common case but have known
gaps a new integrator should know about before relying on them:

- **No TLS.** Connections are established in the clear on `TCP_PORT`
  (`13400`); `TCP_TLS_PORT` (`3496`) is defined per ISO 13400-2 but nothing in
  this crate uses it.
- **No UDP vehicle announcement / discovery.** The server does not send the
  UDP vehicle-announcement broadcast on startup, nor answer vehicle
  identification requests over UDP.
- **The server accepts one TCP connection at a time.** `Server::run_server`'s
  accept loop awaits each client's connection handling to completion before
  calling `accept()` again, so a second client cannot connect while the first
  is still being served.
- **`ClientConnectionInfo::logical_address` is always `0x0000`.** The server
  does not yet track per-connection logical addresses, so this field is a
  placeholder rather than the client's real address.
- The handler passed to `Server::new` is not validated, and a failed
  `accept()` currently panics the server task rather than being handled.

None of this blocks bare-metal or single-client use; it matters if you need
concurrent clients, discovery, or TLS today.

## Quickstart

The protocol core needs no allocator and no I/O: frame a byte buffer with
`try_frame`, then decode the payload with `Payload::decode`. This block is a
copy of the doctest on `try_frame` in [`src/framer.rs`](src/framer.rs) — that
doctest is the canonical, CI-tested version; if the two ever drift, trust the
doctest.

```rust
use simple_doip::{try_frame, messages::Payload};

// A complete DoIP NACK frame: 8-byte header + 1-byte body.
let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
assert_eq!(consumed, 9);

let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
assert!(matches!(payload, Payload::DoIPNack(_)));
# Ok::<(), simple_doip::messages::MessageError>(())
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

## License

Licensed under either of MIT or Apache-2.0 at your option.
