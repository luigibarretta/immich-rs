# immich-rs

`immich-rs` is a planned Rust implementation of a high-integrity import,
archive and migration client for [Immich](https://immich.app/).

The repository is currently an **architecture and test-harness scaffold**. It
does not process media yet. The first production feature may only be enabled
after differential tests against the pinned `immich-go` oracle prove the
relevant behavior on synthetic fixtures.

## Why this project exists

- provide predictable memory bounds and explicit backpressure for very large
  libraries;
- make scan, metadata matching, planning and execution independently testable;
- preserve resumability and idempotency across interrupted imports;
- measure whether Rust improves CPU-bound stages without pretending that it
  can remove network, storage or Immich-server bottlenecks;
- fit the Rust-first application fleet while preserving upstream-compatible
  user outcomes.

This is not currently a promise of a faster drop-in replacement. The upstream
benchmark for `immich-go` already flattens around the server/network limit. The
project therefore uses performance budgets and behavioral parity gates rather
than a language-based speed claim.

## Baselines

| Contract | Pinned baseline |
|---|---|
| Behavioral oracle | `immich-go` v0.32.0, commit `f7d19fce34acd4884ea2c02fc3025706a060afdf` |
| First Immich target | Immich v3.1 API family; exact fixtures are pinned per test suite |
| Rust toolchain | 1.88.0, edition 2024 |
| License | AGPL-3.0-only |

The Go executable is an external, black-box oracle. Its source may be inspected
for behavior and compatibility research, but implementation code must not be
copied into this repository. See [ADR-0003](docs/adr/ADR-0003-licensing-and-provenance.md).

On dev-01, `scripts/verify-local-oracle.sh` proves that the external oracle is
the exact pinned Linux x86-64 binary without contacting Immich or media.

## Workspace

- `immich-cli`: binary and stable user-facing command surface;
- `immich-core`: domain model, plans, events and execution contracts;
- `immich-client`: version-aware Immich HTTP API boundary;
- `immich-sources`: folder/archive/export adapters and metadata reconciliation.

The crate boundaries are dependency rules, not microservices. See
[ADR-0004](docs/adr/ADR-0004-modular-workspace-architecture.md).

## Development gate

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo doc --locked --workspace --no-deps
scripts/check-adrs.sh
```

The current binary intentionally returns exit code 2 for every command except
`--help` and `--version`. This prevents an architecture scaffold from being
mistaken for a functional media tool.

Read [ROADMAP.md](ROADMAP.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the full
[ADR index](docs/adr/README.md) before implementing a feature.

## Project relationship

`immich-rs` is an independent community project. It is not affiliated with or
endorsed by the Immich or immich-go projects. Their names and APIs remain the
property of their respective owners.
