# DoIP

This crate provides an ISO 13400-2 compliant DoIP implementation.
The intention is to open source and publish once the appropriate documentation,
testing and validation is much more complete.

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

For development and testing, enable `client` and `server`:

```sh
cargo test --features client,server
```
