# ADR-0008: Concurrency, backpressure and cancellation

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Parallel discovery, hashing and upload improve throughput until storage,
network or Immich saturates. Unbounded tasks and queues trade speed for memory,
descriptor exhaustion and poor shutdown behavior.

## Decision

Use Tokio for async orchestration and blocking pools for filesystem, archive
and CPU work that cannot yield. The pipeline has separately bounded stages for
discovery, metadata, hashing and server operations. Every queue has an explicit
capacity and applies backpressure.

Concurrency is configured per resource class rather than one global worker
count. Defaults derive conservatively from logical CPUs and never exceed a
documented cap. A cancellation token propagates through all stages. First
interrupt stops new work and checkpoints in-flight results; a second interrupt
may terminate immediately with a distinct exit class.

Adaptive concurrency is deferred until fixed limits are benchmarked and its
stability can be tested. Tasks are owned by structured scopes; detached
background tasks are forbidden.

## Consequences

The scheduler is more explicit than a simple worker pool. Users can tune the
actual bottleneck and cancellation remains deterministic under pressure.

## Verification

Concurrency tests use tiny queue capacities, slow consumers, cancellation and
server throttling. They assert maximum in-flight work, absence of task leaks
and a valid checkpoint after interruption. Benchmarks sweep stage limits rather
than presenting one unexplained concurrency number.
