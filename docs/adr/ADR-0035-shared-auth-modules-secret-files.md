# ADR-0035: Shared auth-modules secret-file primitives

- Status: Accepted
- Date: 2026-11-09
- Owners: project maintainers

## Context

The Web Console carried five near-identical readers for private files
(bootstrap secret, metrics credential, OIDC client secret, TLS private key,
server API key) and six copies of the owner-only permission check, plus two
local constant-time string comparisons. The CLI read `IMMICH_RS_API_KEY_FILE`
with no permission check at all. Every security fix had to be repeated, and
the copies had already drifted in limits, error handling and zeroization.

The fleet library `lib-auth-modules` 0.2.0 provides the same contract once:
`secrets::files::read_private_file*` (no symlink, regular non-empty bounded
file, no group/other bit, owner check, `lstat`/`fstat`/`lstat` device-inode
identity) and `secrets::constant_time_eq`. With only the `secret-files`
feature it adds no transitive crate. Its full OIDC client depends on
`openidconnect` and `jsonwebtoken`, which ADR-0014's minimal-dependency rule
and the EdDSA-only `ring` profile of ADR-0033 do not accept.

## Decision

immich-rs consumes `auth-modules` 0.2.0 as a vendored path dependency
(`vendor/auth-modules`, an exact `git archive` of the pinned tag recorded in
`dependencies/auth-modules.sha`, tree digest in
`dependencies/auth-modules-vendor.sha256`, verified by the `vendored-deps`
ci-local step) with `default-features = false` and only `secret-files`. The
crate is `MIT OR Apache-2.0`, compatible with ADR-0003.

- Web Console private files are read through
  `immich-web/src/bounded_file.rs`, which delegates to
  `read_private_file_with` on Unix. Public files (CA bundles, certificates)
  and every file on Windows keep one local bounded regular-file reader with
  the same identity checks; Windows has no owner-only mode bits, as before.
- The CLI reads API key files with `read_private_string_with`; surrounding
  ASCII whitespace is trimmed. Windows keeps the bounded regular-file check.
- On Linux the file owner must be the effective UID. Other Unix targets
  cannot resolve it without `unsafe`, so they skip only the owner check.
- Session, pairing and OIDC login tokens are compared with
  `secrets::constant_time_eq`.

The local OIDC client (discovery, JWKS, EdDSA verification, PKCE) remains
app-local under ADR-0014 and ADR-0033. Forwarded-header client-IP handling is
not adopted: the console trusts only the immediate TCP peer (ADR-0034).

## Consequences

Private-file hardening has one implementation shared with the rest of the
fleet. The CLI now rejects API key files that are group/other accessible or,
on Linux, owned by another user: a Compose secret must be prepared as UID
65532 with mode `0400`. Refreshing the library is a pin, tree and digest
change reviewed like any dependency update.

## Verification

`check-vendored-dep` proves the vendored tree matches the pin and digest.
Unit tests in `bounded_file_tests.rs` and the CLI `network_tests.rs` reject
group/other permissions, symlinks, directories, empty and oversized files and
accept owner-only files. `cargo deny` checks the dependency's license and the
exact-version path entry.
