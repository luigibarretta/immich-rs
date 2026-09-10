# Changelog

All notable changes will be documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
Semantic Versioning.

## [0.1.0-rc.3] - 2026-09-10

### Changed

- Superseded the signed but unpublished `0.1.0-rc.2` candidate after its
  fail-closed release workflow rejected incomplete OCI output.

### Fixed

- Made filesystem identity and TOML test fixtures portable across native
  Windows volumes and paths.
- Isolated each multiarch OCI build outside the source checkout so both CLI
  and Web Console release products can be staged without dirty-tree failures.

## [0.1.0-rc.2] - 2026-09-10

### Added

- Optional authenticated `immich-rs-web` operator console with bounded jobs,
  immutable plan review, offline dry-run and exact single-use apply grants.
- Direct TLS 1.3 and OIDC authorization-code plus PKCE LAN mode with private
  address pinning, restart-ephemeral sessions and fail-closed key rotation.
- Separate downloadable Web Console binaries for all five native targets and
  a separate attested amd64/arm64 OCI archive in the signed release bundle.
- Label-free aggregate metrics with browser authentication or an optional
  file-backed machine bearer constrained by immediate-peer CIDRs.
- CI-enforced disposable Web Console authentication, shutdown, cold backup,
  restore and restart evidence using only a synthetic loopback source.

### Changed

- Published a fresh comparable benchmark set for folder planning, folder
  upload and complete Google Takeout import; only the exact Takeout result
  supports a current speed claim.
- Made Python tooling, fixtures and native tests portable across Linux, macOS
  ARM64 and Windows x86-64 host runners.

### Fixed

- Sanitized benchmark child environments so release credentials cannot alter
  tool identity inspection.

## [0.1.0-rc.1] - 2026-09-04

### Added

- Phase 0 architecture, synthetic fixtures, oracle, mock, benchmark and CI
  foundations.
- Deterministic folder, Google Takeout and Apple Photos read-only normalized
  planners.
- Loopback-only idempotent folder upload for disposable Immich instances.
- Loopback-only verified original-byte Immich archive with idempotent resume.
- Differential, disposable, paired benchmark, large synthetic soak and
  authorized private read-only shadow evidence.
- Strict layered CLI/environment/TOML configuration and a hardened,
  attested amd64/arm64 OCI and Compose packaging contract.
- Verified remote HTTPS for read-only archive and immutable folder upload with
  exact CLI-only production authorization and private-CA support.
- Source-aware Google Takeout import with bounded directory/split-ZIP
  streaming, normalized metadata and albums, effect-level checkpoints and
  disposable postcondition evidence.
- Reproducible paired Takeout plan/import benchmark against the pinned
  immich-go v0.32.0 black-box oracle.
- Source-aware Apple Photos import with preserve-all assets, XMP, Live Photos,
  explicit albums, offline dry-run and resumable directory/split-ZIP apply.
- Bounded Picasa adapter for directory/split-ZIP sources, reviewed
  `.picasa.ini` albums/captions and optional filename-date fallback.
- Disposable Immich-to-Immich migration with a read-only source capability,
  distinct credentials, immutable plans, one-original staging and durable
  effect-level resume.
- Real disposable compatibility gates and paired raw benchmarks for Apple
  Photos, Picasa and two-server migration, with ADR-0012-scoped synthetic
  performance claims and no generalized or production claim.
This is the first signed release candidate. Production migration, destructive
maintenance and compatibility beyond the documented Immich v3.1.x surfaces
remain out of scope.
