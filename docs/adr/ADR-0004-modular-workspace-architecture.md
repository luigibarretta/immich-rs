# ADR-0004: Modular workspace architecture

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Media discovery, metadata reconciliation, API transport and CLI presentation
change for different reasons. A single crate would permit source-format and
HTTP concerns to leak into every feature; premature services would add runtime
and deployment cost without isolation value.

## Decision

Use one Rust workspace and one process with four initial crates:

- `immich-cli`: argument/config parsing, presentation and exit codes;
- `immich-core`: source-neutral domain types, immutable plans, progress events
  and execution contracts;
- `immich-client`: all Immich HTTP/API/version behavior;
- `immich-sources`: folder, archive and export adapters plus metadata matching.

Dependencies point inward: CLI may depend on all libraries; sources and client
may depend on core; core depends on neither. Source adapters never call Immich
directly. Libraries do not read process environment, print or exit. Crate
splits may be added only when they enforce a real dependency or build boundary.

## Consequences

The tool remains operationally simple while tests can replace sources and the
server independently. Some types must be designed at explicit boundaries and
cannot rely on ad-hoc global state.

## Verification

Workspace manifests encode allowed dependencies. Architecture tests or a
dependency-policy script are added before the first cross-crate dependency.
Review rejects direct HTTP in sources and CLI/environment concerns in library
crates.
