# ADR-0037: Rust 1.96 toolchain

- Status: Accepted
- Date: 2026-11-11
- Owners: project maintainers
- Supersedes clauses in: ADR-0014, ADR-0025 and ADR-0033

## Context

ADR-0014 fixes the workspace MSRV at Rust 1.88, ADR-0025 requires every native
release job to use the locked Rust 1.88.0 toolchain and ADR-0033 admits Web
Console dependencies only when they are compatible with Rust 1.88.

RUSTSEC-2026-0459 (CVE-2026-119375) affects the SQLite bundled by
`libsqlite3-sys` before 0.39.0. The fix ships with `rusqlite` 0.41.1
(`libsqlite3-sys` 0.39.1, SQLite 3.54.1), which requires Rust 1.89. The
advisory was accepted as a temporary exception until 2026-12-15
(`docs/security/advisory-exceptions.md`) and can only be closed with a
toolchain bump.

## Decision

This ADR supersedes only the toolchain clauses of ADR-0014 (MSRV 1.88),
ADR-0025 (locked Rust 1.88.0 for every release job) and ADR-0033 (dependencies
compatible with Rust 1.88). Everything else in those ADRs is unchanged.

- `rust-toolchain.toml` pins Rust 1.96.1; the workspace `rust-version` is
  1.96, so the MSRV keeps matching the pinned toolchain minor.
- Every Gitea and GitHub workflow installs exactly 1.96.1 (GitHub actions
  pinned by commit SHA), and the container builder images are
  `rust:1.96.1-bookworm` pinned by index digest.
- Dependencies are admitted only when compatible with Rust 1.96, in addition
  to the existing ADR-0003 and ADR-0013 policy.
- Lints newly enabled by the toolchain are fixed in code; the workspace keeps
  denying `clippy::all`, `pedantic` and `nursery` without new allowances.

## Consequences

`rusqlite` moves to 0.41.1 and the RUSTSEC-2026-0459 exception is removed from
`deny.toml` and the `audit-lockfiles` step. Contributors and runners need Rust
1.96.1; build caches keyed on 1.88.0 are invalidated once. Committed benchmark
evidence keeps recording the `rustc` that produced it.

## Verification

`cargo fmt`, `clippy` with warnings denied, workspace tests, rustdoc,
`cargo deny check` and the RustSec lockfile audit run on 1.96.1 without an
advisory exception.
