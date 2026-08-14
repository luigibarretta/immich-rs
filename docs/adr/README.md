# Architecture decision records

Accepted ADRs are binding for implementation and review. A later decision does
not edit history: it adds a new ADR that names and supersedes the earlier one.

Statuses:

- **Proposed**: open for review; implementation must not depend on it yet.
- **Accepted**: current decision.
- **Superseded**: replaced by a named later ADR.
- **Rejected**: considered and intentionally not selected.

Every ADR contains Context, Decision, Consequences and Verification sections.
Run `scripts/check-adrs.sh` before committing.

## Index

| ADR | Decision | Status |
|---|---|---|
| [0001](ADR-0001-architecture-decision-process.md) | Architecture decision process | Accepted |
| [0002](ADR-0002-product-scope-and-compatibility.md) | Product scope and compatibility target | Accepted |
| [0003](ADR-0003-licensing-and-provenance.md) | Licensing and provenance | Accepted |
| [0004](ADR-0004-modular-workspace-architecture.md) | Modular workspace architecture | Accepted |
| [0005](ADR-0005-cli-and-configuration-contract.md) | CLI and configuration contract | Accepted |
| [0006](ADR-0006-immich-api-boundary.md) | Immich API boundary and version policy | Accepted |
| [0007](ADR-0007-streaming-io-and-memory-bounds.md) | Streaming I/O and memory bounds | Accepted |
| [0008](ADR-0008-concurrency-backpressure-and-cancellation.md) | Concurrency, backpressure and cancellation | Accepted |
| [0009](ADR-0009-source-adapters-and-metadata-reconciliation.md) | Source adapters and metadata reconciliation | Accepted |
| [0010](ADR-0010-plans-checkpoints-retries-and-idempotency.md) | Plans, checkpoints, retries and idempotency | Accepted |
| [0011](ADR-0011-differential-and-golden-testing.md) | Differential and golden testing | Accepted |
| [0012](ADR-0012-performance-measurement-and-budgets.md) | Performance measurement and budgets | Accepted |
| [0013](ADR-0013-security-privacy-and-observability.md) | Security, privacy and observability | Accepted |
| [0014](ADR-0014-platform-release-and-supply-chain.md) | Platform, release and supply chain | Accepted |
| [0015](ADR-0015-phased-delivery-and-production-cutover.md) | Phased delivery and production cutover | Accepted |

## Template

```markdown
# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Owners: project maintainers

## Context

## Decision

## Consequences

## Verification
```
