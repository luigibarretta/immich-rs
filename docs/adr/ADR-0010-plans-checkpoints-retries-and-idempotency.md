# ADR-0010: Plans, checkpoints, retries and idempotency

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Large imports are interrupted by networks, reboots, server upgrades and user
cancellation. Repeating a partially completed run must not create duplicates,
lose metadata or conceal partial failure.

## Decision

Execution is split into an immutable normalized plan and an apply phase. Plans
carry a schema version, source fingerprint, relevant configuration digest and
server compatibility facts. Apply writes an append-only checkpoint journal to
a local SQLite database using durable transactions. Checkpoints reference
stable operation IDs derived from the plan, not queue order.

Server mutations use native idempotency/duplicate contracts where available
and reconcile after uncertain outcomes. Retries are bounded, jittered and only
for typed transient failures. Authentication, validation, ambiguity and
invariant failures never retry automatically. A retry budget exists per
operation and per run.

Resume refuses a changed source, incompatible configuration or plan schema
unless an explicit migration can prove safety. Reports distinguish succeeded,
skipped, duplicate, retried, failed and indeterminate operations.

## Consequences

SQLite and plan schemas become compatibility surfaces requiring migrations.
The tool can resume safely and explain partial outcomes rather than restarting
blindly.

## Verification

Fault-injection tests terminate the process before request, during body upload,
after server commit and before checkpoint commit. Repeated apply must converge
without duplicate assets. Journal corruption, disk-full and schema mismatch
fail closed with recoverable diagnostics.
