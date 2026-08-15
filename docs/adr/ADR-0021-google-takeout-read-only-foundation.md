# ADR-0021: Google Takeout read-only foundation

- Status: Accepted
- Date: 2026-08-15
- Owners: project maintainers

## Context

Phase 3 starts after the folder planning and disposable folder upload gates.
Google Takeout adds format-specific layout validation and JSON metadata
matching, but the first vertical must remain small enough to review against a
synthetic black-box observation. Supporting archives, albums and all metadata
semantics at once would hide reconciliation errors behind a broad interface.

## Decision

The first Google Takeout vertical accepts one decompressed directory layout: a
caller-selected root containing the real directories `Takeout/Google Photos`.
The public read-only command is `immich-rs plan google-takeout`. It emits only
`normalized-plan-v1`; the source descriptor uses the additive
`google_takeout` source kind. No Takeout plan can enter the Phase 2 folder
upload planner or executor.

The adapter reuses bounded filesystem discovery, content hashing, path
normalization and common collision handling. A Google JSON sidecar is parsed
from a regular file no larger than 256 KiB. This slice recognizes the bounded
top-level `title` string and associates the sidecar only with the unique media
file in the same directory whose filename equals that title. The rule is
`META_GOOGLE_TAKEOUT_TITLE_V1`. Invalid, oversized and ambiguous JSON fails
closed with stable rule IDs; a sidecar without a matching media file remains
an explainable warning.

ZIP input, split archives, truncated supplemental-metadata filenames, albums,
descriptions, coordinates, timestamps, timezone conversion, people and
partner metadata are unsupported in this vertical. They must not be silently
interpreted and require later evidence and decisions. Upload, delete, replace
and metadata mutation are unchanged and unavailable for Google Takeout.

Compatibility evidence uses only the verified immich-go v0.32.0 executable as
a black-box oracle, a generated synthetic Takeout fixture and the loopback
mock. The oracle binary is not redistributed. Its previously pinned dry-run
job-resume defect remains contained and must not authorize asset mutations.

## Consequences

This provides an auditable Phase 3 foundation without pretending to support a
complete Takeout export. Metadata parsing is memory-bounded independently of
media size, and unchanged inputs produce byte-identical plans. Users receive
an explicit source error for unsupported layouts or malformed recognized
metadata instead of partial compatibility.

## Verification

Golden CLI tests cover the synthetic layout and exact normalized plan. Unit
and property tests cover title matching, malformed and oversized JSON,
ambiguity, deterministic ordering and cancellation. The architecture check
proves the Takeout CLI module cannot reference the Immich client, executor,
server configuration or API keys. A differential case compares the same
synthetic corpus with immich-go v0.32.0 and rejects unexpected mutations.
