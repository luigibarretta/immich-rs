# Advisory exceptions

Every RustSec advisory accepted in `deny.toml` and in the `audit-lockfiles` step of
`ci-local.json` is listed here with its impact, controls and exit path. An exception in
`ci-local.json` carries an `until` date; the audit fails again after it.

## `RUSTSEC-2026-0459` / `libsqlite3-sys` bundled SQLite (CVE-2026-119375)

Accepted: 2026-11-09, until 2026-12-15.

Impact: SQLite before 3.54.0 can return uninitialized page-cache memory when it opens a
database together with a crafted `-wal` file of a different page size. immich-rs bundles
SQLite through `rusqlite` 0.37 (`libsqlite3-sys` 0.35) and opens only its own databases
(import journal, executor journal, web history) inside its private state directory. A
crafted database and `-wal` pair therefore needs write access to that directory, which
already grants control of the service state.

Exit path: `rusqlite` 0.41 (`libsqlite3-sys` 0.39, SQLite 3.54) requires Rust 1.89, while
`rust-toolchain.toml` pins 1.88.0. Bump the toolchain (workflows, Containerfile image
digests, README, ADRs 0014/0025/0033, clippy re-baseline), upgrade `rusqlite`, and remove
both exception entries.
