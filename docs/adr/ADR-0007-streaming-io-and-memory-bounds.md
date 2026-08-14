# ADR-0007: Streaming I/O and memory bounds

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The tool must handle multi-gigabyte videos, many archives and libraries with
tens of thousands of assets. A Rust implementation that buffers full media or
unbounded metadata could use more memory than the Go oracle despite the
language change.

## Decision

Media bytes flow as streams from a source handle through optional hashing and
multipart upload. Whole media files are never collected into memory. Archive
entries are consumed with bounded buffers; extraction to disk is explicit and
budgeted. Metadata records have size limits and parsers reject pathological
nested or oversized input.

Discovery produces compact descriptors and spills plans/indexes to a durable
store when a configured memory budget would be exceeded. Buffer sizes and the
maximum number of open files are centralized configuration with conservative
defaults. Temporary files use a user-selected workspace, atomic publication
and deterministic cleanup/recovery rules.

## Consequences

Some APIs require asynchronous readers and reopenable source handles instead
of convenient byte vectors. The design can process assets larger than RAM and
provides a measurable memory ceiling.

## Verification

Tests stream sparse/virtual large files while asserting bounded RSS and buffer
allocations. Archive bombs, oversized metadata and file-descriptor pressure are
fault cases. Benchmarks report peak RSS and open descriptors, not only wall
time.
