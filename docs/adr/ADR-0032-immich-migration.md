# ADR-0032: Immich-to-Immich migration

- Status: Accepted
- Date: 2026-08-24
- Owners: project maintainers

## Context

The pinned immich-go v0.32.0 black-box exposes `upload from-immich`. immich-rs
currently has a read-only Immich inventory/archive capability and a separate
plan-bound upload/import capability, but it has no command that binds two
different servers into one migration contract.

A migration has a higher authorization and failure-recovery risk than a local
import: the source and destination can be confused, two credentials are
required, remote bytes can change between planning and apply, and a committed
destination upload can lose its response.

## Decision

Implement a distinct versioned migration plan with two immutable server
identities. The capability contract is:

1. the source client is read-only by type and may only inventory/download
   originals plus declared metadata and owned album membership;
2. the destination client is separate and cannot be constructed until the
   exact plan digest, maximum mutation count and hashed backup reference are
   confirmed;
3. source and destination origins and credentials are distinct, redacted and
   rejected when ambiguously configured;
4. apply downloads and verifies at most one asset into private plan-bound
   staging, uploads it, records the effect, then removes it before continuing;
5. no whole-library or whole-media buffering, unbounded concurrency or direct
   server-to-server credential forwarding is permitted;
6. checkpoint state binds both server identities, every source asset identity,
   destination effects and the backup digest;
7. cancellation, retry, disconnect and lost-response handling reuse bounded
   import semantics and must converge without blind duplicate mutation;
8. no source mutation and no destination delete, replace, trash, people, stack
   or independent maintenance command is introduced.

The first compatibility matrix covers timeline assets, original bytes,
capture time, description, location and owned albums. Archived/trashed assets,
partners, people, tags, favorites, ratings, stacks and filters remain outside
the declared matrix until a superseding ADR.

The gate requires two disposable Immich instances, aggregate postconditions,
exact zero-resource cleanup, a pinned black-box `from-immich` differential and
comparable raw benchmarks. Production endpoints are not authorized by this
ADR alone.

## Consequences

- Migration cannot be represented as a folder upload or a local archive plus
  an implicit second command.
- The dependency graph must prove that the source side has no mutation
  capability.
- Supporting additional remote state expands the mutation budget and requires
  a new reviewed compatibility boundary.
- No Go source may be inspected, copied, translated or linked.

## Verification

Unit and mock tests prove that the source capability cannot issue mutations,
the destination cannot be constructed during planning, two-server identity is
bound into the plan and checkpoint, staging contains at most one verified
asset, and retry, cancellation, resume and lost responses converge by durable
effect.

A disposable gate starts two isolated Immich stacks, migrates only synthetic
assets and owned albums, verifies source immutability and destination aggregate
postconditions, captures comparable raw measurements, and removes every
labelled resource. The black-box immich-go v0.32.0 differential and an
authorized non-production export remain required external evidence before the
migration compatibility gate can be declared complete.
