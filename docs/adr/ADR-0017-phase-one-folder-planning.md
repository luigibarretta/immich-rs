# ADR-0017: Phase-one folder planning policy

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The first executable vertical must turn a folder into a stable read-only plan
without acquiring an Immich mutation capability. Filesystem ambiguity,
platform-specific paths and source changes must have explicit outcomes before
an apply phase can exist.

## Decision

`immich-sources` recursively enumerates real directories in normalized
deterministic order. It never follows symbolic links. Regular supported media
are hashed one at a time through a configurable bounded buffer. Total entries,
entries per directory and portable path bytes have hard limits; reaching a
limit fails closed or emits the corresponding bounded diagnostic.

Supported media and sidecars are both streamed and included in the source
identity, so unchanged source content is the idempotency boundary. Portable
paths use Unicode NFC. Native paths that normalize to one portable
path are removed as an ambiguous group and emit `FS_UNICODE_COLLISION_V1`.
Paths that differ only by case remain visible but emit
`FS_CASE_COLLISION_V1`. Duplicate basenames in different directories remain
distinct and produce an explainable warning. Unreadable files, special entries
and source identity changes are explicit rule-ID findings.

An exact `<media-name>.json` sidecar attaches to that media. XMP and remaining
sidecars use an unambiguous same-directory basename match, preferring the
single image of a live-photo pair. Orphan and ambiguous sidecars are not
silently selected. One image and one MOV with the same directory and basename
form a live-photo pair with a stable derived pair ID.

The public command is only `immich-rs plan folder`. The CLI depends on
`immich-core` and `immich-sources`, not `immich-client`; no upload, delete,
replace or metadata-mutation command exists. Repeating a scan over unchanged
bytes and configuration must produce byte-identical JSON. Cancellation returns
no partial plan.

Differential compatibility is measured from normalized black-box observations,
not Go implementation details. The declared generic synthetic JSON sidecar is
an intentional divergence: immich-rs retains an explainable metadata candidate
while immich-go v0.32.0 reports that the JSON was not produced by immich-go.
The oracle's five job-resume requests during dry-run are classified as a pinned
oracle defect and contained by the loopback mock.

## Consequences

Phase 1 is useful for inspection and compatibility work but cannot modify an
Immich server. Symlinks are represented as warnings rather than traversed.
Hard entry limits provide a current memory ceiling; scalable spill storage
remains required before advertising unbounded-library support. New matching
heuristics require stable rule IDs, goldens and differential evidence.

## Verification

Golden CLI tests compare exact bytes. Unit and property tests cover creation
order, buffer sizes, Unicode, collisions, sidecars, sidecar content identity,
live photos, symlinks, limits, unreadable files, source changes and mid-scan
cancellation.
`scripts/check-architecture.py` proves the Phase 1 dependency graph cannot
reach the Immich client. `scripts/compare-oracle.py` enforces every declared
matrix row against the pinned oracle and synthetic mock.
