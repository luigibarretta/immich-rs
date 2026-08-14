# ADR-0014: Platform, release and supply chain

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The reference tool is used across Linux, macOS and Windows. A local-only Rust
binary would not be a credible replacement, while unpinned tools and opaque
release builds would weaken reproducibility.

## Decision

The workspace uses Rust edition 2024 with MSRV 1.88. Supported release targets
are Linux x86-64/arm64, macOS x86-64/arm64 and Windows x86-64. Platform-specific
filesystem behavior is isolated behind tested abstractions.

Dependencies are minimal, use rustls instead of native TLS where practical and
are locked for applications and CI. The repository will enforce vulnerability,
license, source and duplicate-version policy before adding its first external
runtime dependency. Generated code records generator/spec digests.

Releases are immutable signed tags built only after the full compatibility
gate. Artifacts include SHA-256 checksums, SBOM, build provenance, changelog and
supported Immich/format matrix. No `latest` artifact is used by production
automation. Containers, if added later, are secondary packaging and run
non-root with a read-only root filesystem.

## Consequences

Cross-platform CI and release provenance add cost. Users receive auditable
artifacts and platform behavior cannot silently depend on the developer host.

## Verification

CI builds/tests the supported matrix before release enablement. Release jobs
rebuild from a clean tag and verify the tree. Dependency-policy, audit, SBOM and
provenance jobs are mandatory before version 0.1.0.
