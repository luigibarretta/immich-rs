# ADR-0003: Licensing and provenance

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The reference implementation, immich-go, is AGPL-3.0 licensed. This project is
informed by its documented behavior, CLI and black-box execution. Claiming a
clean-room permissive rewrite while routinely consulting that project would be
misleading and would create avoidable provenance risk.

## Decision

`immich-rs` is licensed AGPL-3.0-only. New code is authored in Rust from the
decisions and tests in this repository. No immich-go implementation code is
copied, mechanically translated or committed. The pinned immich-go executable
is invoked only as an external oracle and is not redistributed in immich-rs
release artifacts.

Third-party fixture and generated-code provenance must be documented beside
the artifact. Dependencies must be compatible with AGPL distribution. A future
license change requires a new ADR and a complete contributor/copyright review.

## Consequences

The project adopts strong copyleft and avoids a false independence claim.
Some proprietary embedding use cases are excluded. Black-box differential
testing remains allowed without making the Go binary part of this work.

## Verification

Every crate declares `AGPL-3.0-only`; the repository contains the full license.
CI will add license/dependency checks before external dependencies or releases
are introduced. Review rejects copied Go structure, comments or implementation
fragments without explicit provenance and license analysis.
