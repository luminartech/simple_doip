# Contributing

Pull requests are welcome, as are bug reports and questions in the issue
tracker.

[`ARCHITECTURE.md`](ARCHITECTURE.md) is the place to start for anything beyond
a small fix. It covers the feature-gated layering, the sans-io framing/decode
seam, the error taxonomy, and — in its last section — the invariants to
preserve when changing the crate.

## Building and testing

The minimum supported Rust version is **1.88**. `default = []`, so a bare
`cargo test` exercises only the `no_std` core:

```sh
cargo test --all-features                       # everything
cargo test --features client,server             # the async layers
cargo test --no-default-features                # the no_std core alone
```

The `no_std` core must keep building for a bare-metal target:

```sh
rustup target add thumbv7em-none-eabihf
cargo check --no-default-features --target thumbv7em-none-eabihf
```

CI additionally checks each feature individually, `cargo fmt`, and
`cargo clippy --all-features -- -D warnings -Dclippy::pedantic`, and builds the
documentation with `RUSTDOCFLAGS=-D warnings`. Running the three commands above
plus `cargo fmt` and clippy locally will catch almost everything before it gets
there.

## Wire-format changes

`tests/golden_vectors.rs` decodes and re-encodes byte sequences held as `.hex`
files in `tests/golden/`. A change that alters the bytes on the wire should
either fail one of those vectors or add a new one — a round-trip test written
against the crate's own output passes whenever `encode` and `decode` share the
same misreading.

## Commits and pull requests

Commit subjects follow [Conventional Commits](https://www.conventionalcommits.org/)
(`feat:`, `fix:`, `docs:`, `build:`, `chore:`, with a `!` for a breaking
change), because the changelog is organized around them. Say what changed and
why in the body.

## Licensing

Contributions are dual-licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option,
matching the crate itself. By opening a pull request you agree that your
contribution may be distributed under those terms.
