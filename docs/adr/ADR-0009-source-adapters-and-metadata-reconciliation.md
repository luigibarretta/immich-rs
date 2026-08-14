# ADR-0009: Source adapters and metadata reconciliation

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Folder trees, ZIP collections, Google Takeout and iCloud exports expose
different paths and sidecars. Filename truncation, duplicate basenames,
timezone changes, edits, live photos and split archives make heuristic matching
the riskiest compatibility surface.

## Decision

Each format implements a source adapter that emits source-neutral candidate
assets and metadata candidates. A deterministic reconciliation engine ranks
matches using explicit evidence such as normalized path, original filename,
archive identity, timestamp proximity, media identity and sidecar references.

Rules have stable identifiers and emit explainable evidence. Ambiguous matches
never silently choose a winner: they become plan warnings or errors according
to policy. Original and edited variants, live-photo pairs, sidecars, albums,
tags, descriptions, coordinates and timezone handling remain distinct domain
concepts until planning resolves them.

Filesystem and archive adapters share a virtual path abstraction so split
archives can be tested without extracting them. Unicode normalization and
case-sensitivity are explicit per source, never inherited accidentally from the
host filesystem.

## Consequences

Metadata matching takes more modeling than filename lookup, but decisions are
auditable and format-specific behavior stays out of the uploader.

## Verification

Golden fixtures cover truncation, duplicate names, missing sidecars, split
archives, Unicode, case collisions, timezone boundaries, edits and live-photo
pairs. Normalized plans record rule IDs and evidence. Differential tests compare
supported outcomes with immich-go.
