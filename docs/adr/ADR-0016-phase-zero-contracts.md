# ADR-0016: Phase-zero fixture and normalized-plan contracts

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The differential harness needs portable inputs and outputs before folder
behavior can be compared. Committing opaque media, machine-local paths or
oracle presentation logs would make parity neither safe nor reproducible.

## Decision

Synthetic fixtures use `fixture-manifest-v1` JSON. A manifest declares its
schema, synthetic provenance, deterministic generator and every materialized
file. Media are generated outside the repository from bounded declarative
recipes; no personal or externally sourced media are accepted by default.

Read-only discovery emits `normalized-plan-v1` JSON using the source-neutral
types in `immich-core`. Portable relative paths are NFC-normalized and sorted.
Content and source identities are SHA-256 values. Decisions and diagnostics
carry stable rule IDs. Absolute source paths, timestamps and server identifiers
are excluded from the plan.

Oracle observations use `oracle-observation-v1` JSON. The runner verifies the
configured immich-go version and executable digest before materializing a
synthetic fixture. It captures process streams, exit status and mock-server
requests, then removes volatile timestamps, generated identifiers and
non-semantic ordering. The executable remains external and is never copied
into repository artifacts.

Repository checks fail closed on undeclared fixture files, missing provenance,
digest drift, credential-shaped values, production host patterns and personal
metadata keys. New fixture or plan schemas require a new version and migration
documentation; existing expected outputs are immutable compatibility evidence.

## Consequences

Fixtures remain reviewable text and can be recreated on every platform.
Differential results compare stable domain facts instead of CLI prose. Adding a
fixture requires an explicit manifest and expected-plan digest, which is
deliberate review friction.

## Verification

`scripts/check-fixtures.py` validates schemas, provenance, paths, digests and
forbidden-data canaries. Materializer tests prove deterministic output and path
containment. Core tests round-trip and validate normalized plans. Differential
tests verify that oracle observations contain only declared synthetic fixture
paths and normalized mock-server outcomes.
