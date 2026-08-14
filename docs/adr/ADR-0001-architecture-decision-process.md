# ADR-0001: Architecture decision process

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

`immich-rs` must preserve subtle media-import behavior while changing language,
runtime and internal architecture. Unrecorded decisions would make later
compatibility and performance work impossible to audit.

## Decision

Material decisions are recorded before implementation. Accepted ADRs are
immutable historical records. A change creates a new ADR that explicitly
supersedes the old one. Small implementation details remain in code and tests;
cross-cutting contracts, irreversible choices and security boundaries require
an ADR.

An ADR is accepted when the repository owner merges it into `main`. Proposed
ADRs cannot authorize production behavior.

## Consequences

The project carries more documentation early, but another session can resume
without reconstructing intent from code. Review can distinguish deliberate
trade-offs from accidental drift.

## Verification

CI runs `scripts/check-adrs.sh`, which requires the catalog, a recognized
status and the standard decision sections. Review checks that code does not
contradict an accepted ADR.
