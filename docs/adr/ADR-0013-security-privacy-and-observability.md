# ADR-0013: Security, privacy and observability

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The client handles API keys, personal photos, paths, coordinates, faces and
descriptions. Detailed logs useful for debugging can themselves become a
privacy incident. Automated tests also need enough telemetry to diagnose
compatibility and performance failures.

## Decision

Secrets use redacting wrappers and are never included in `Debug`, errors,
events or metrics. TLS verification is on by default. Insecure transport is an
interactive-only explicit override and cannot be enabled by a persisted default
or background automation.

Structured local events have stable names, severity and operation IDs. Default
logs omit full paths, filenames, metadata values, coordinates, asset hashes and
server response bodies; an explicit diagnostic mode may reveal bounded local
details with a warning and never reveals credentials. Metrics are aggregate and
low-cardinality. No telemetry leaves the machine unless the user configures an
exporter.

Input paths are treated as untrusted. Archive traversal, symlinks, device files,
oversized metadata and decompression ratios are constrained. Mutation
capabilities are separate types from read-only capabilities and dry-run cannot
obtain them.

## Consequences

Some failures require a user-generated diagnostic bundle rather than verbose
default logs. Privacy and least privilege remain enforceable below the CLI.

## Verification

Secret-canary tests search stdout, stderr, events, snapshots and diagnostics.
Archive/path adversarial suites cover traversal and bombs. Tests prove that
read-only/dry-run dependency graphs contain no mutation implementation. Security
review is required before the first production mutation phase.
