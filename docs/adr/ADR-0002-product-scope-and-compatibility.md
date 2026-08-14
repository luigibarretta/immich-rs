# ADR-0002: Product scope and compatibility target

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

`immich-go` covers folders, Google Takeout, iCloud exports, archives, albums,
tags, stacks, duplicates and maintenance operations. Attempting all behavior in
one rewrite would hide regressions and delay useful evidence.

## Decision

`immich-rs` is an import, archive and migration CLI for Immich. Compatibility
means equivalent user-visible outcomes for a declared matrix, not identical
internal algorithms or byte-for-byte logs.

The order is read-only scan/plan, folder upload, Google Takeout, iCloud/Photos,
then archive and maintenance commands. A capability not listed as supported
must fail explicitly; silent partial compatibility is forbidden.

The initial server target is the Immich v3.1 API family. Each test suite pins
the exact server/OpenAPI fixture it exercises. The project is not an Immich
server, UI, photo manager or general backup system.

## Consequences

The first releases expose fewer commands than immich-go. Users gain a precise
compatibility matrix and fail-closed behavior. New formats arrive as adapters
without destabilizing existing ones.

## Verification

`ROADMAP.md` defines phase gates. Documentation and `--help` label unsupported
features. Compatibility tests bind each supported behavior to a fixture,
oracle observation and expected normalized plan/result.
