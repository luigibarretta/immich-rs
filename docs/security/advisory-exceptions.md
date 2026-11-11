# Advisory exceptions

Every RustSec advisory accepted in `deny.toml` and in the `audit-lockfiles` step of
`ci-local.json` is listed here with its impact, controls and exit path. An exception in
`ci-local.json` carries an `until` date; the audit fails again after it.

There are no active exceptions.

## Closed

### `RUSTSEC-2026-0459` / `libsqlite3-sys` bundled SQLite (CVE-2026-119375)

Accepted: 2026-11-09, until 2026-12-15. Closed: 2026-11-11.

SQLite before 3.54.0 can return uninitialized page-cache memory when it opens a database
together with a crafted `-wal` file of a different page size. immich-rs bundled SQLite
through `rusqlite` 0.37 (`libsqlite3-sys` 0.35) for its own state databases only.

Closed by the Rust 1.96.1 toolchain bump (ADR-0037) and `rusqlite` 0.41.1
(`libsqlite3-sys` 0.39.1, SQLite 3.54.1); both exception entries were removed.
