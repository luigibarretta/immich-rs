# ADR-0012: Performance measurement and budgets

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Rust does not guarantee a faster end-to-end media import. Upstream immich-go
measurements for 2,846 items fall from about 136 seconds with one worker to 51
seconds with 12, then flatten around 50–52 seconds at 24–48 workers. Network and
server limits dominate after concurrency saturates them.

## Decision

Performance claims require paired runs using the same hardware, source,
filesystem cache state, network, Immich version, server state and concurrency
budget. Record wall time, user/system CPU, peak RSS, bytes read/written,
requests, retries, server CPU and server I/O where available.

Maintain separate suites for scan/plan CPU work, metadata-heavy archives,
streaming upload and end-to-end import. Report distributions across repeated
runs, not a single best sample. Compare both default settings and matched fixed
concurrency.

Initial acceptance budgets are:

- no supported corpus may be more than 10% slower than the oracle at p95
  without an accepted correctness trade-off;
- peak client RSS must not exceed the configured budget and should be at least
  25% lower on metadata-heavy large-library tests before memory efficiency is
  advertised;
- an end-to-end speed claim requires at least 10% median improvement outside
  measurement noise;
- CPU-stage improvements are reported separately and never presented as upload
  improvements.

## Consequences

Some optimizations will be rejected because they only move cost to Immich or
increase memory. Results are slower to produce but remain honest and
reproducible.

## Verification

Benchmark artifacts include environment manifests, command lines, fixture
digests and raw samples. CI runs small regression budgets; scheduled/manual
hardware runs produce release evidence. Reviews reject unsupported percentage
claims.
