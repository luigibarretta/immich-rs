# ADR-0022: Complete Google Takeout read-only planning

- Status: Accepted
- Date: 2026-08-15
- Owners: project maintainers
- Supersedes: ADR-0021

## Context

ADR-0021 established a deliberately narrow decompressed Google Takeout
adapter. The complete Phase 3 read-only gate additionally needs split ZIP
exports, supplemental filename recovery, normalized metadata and albums. These
features cross the source abstraction and normalized-plan schema boundaries,
so implementing them as unrelated matching heuristics would make ambiguity,
resource use and schema compatibility difficult to audit.

## Decision

### Input and archive boundary

`plan google-takeout` accepts either one decompressed export root or one to 64
independent ZIP files. Directory and ZIP inputs cannot be mixed. Multiple ZIPs
represent Google Takeout parts, not PKZIP multi-disk archives, and their CLI
order has no semantic effect.

ZIP entries are read through a virtual path adapter and are never implicitly
extracted. Only stored and DEFLATE entries are accepted. Encrypted entries,
unsafe or non-Unicode paths, symlinks, devices, unsupported compression,
invalid CRCs, entry-count excess and suspicious compression ratios fail
closed. Entry data is consumed through the configured scan buffer, one open
archive and one entry at a time. JSON retains its independent 256 KiB limit.
The ZIP implementation is pinned with minimal features and remains subject to
the repository license, advisory and duplicate-dependency gates.

Logical paths are NFC-normalized. Identical copies at the same logical path
across parts are coalesced; conflicting copies are an error and are excluded.
Identical media content at different Takeout paths is one logical asset. The
canonical path prefers `Photos from YYYY` and is otherwise lexicographic;
aliases remain explainable evidence and album membership is preserved.

### Reconciliation and normalized metadata

Complete Takeout plans use `normalized-plan-v2`; Phase 1 folder and Phase 2
upload contracts remain immutable on version 1. Version 2 adds optional
source-neutral normalized metadata to an asset: description, UTC capture
instant, geographic coordinates and a sorted set of album names. Version 1
deserialization remains supported, while version 1 plans containing version 2
fields fail validation. Takeout plans cannot enter the folder upload planner
or executor.

Asset JSON is associated in this deterministic order:

1. a valid top-level `title` selecting exactly one same-directory media path;
2. a supplemental sidecar basename, after removing `.supplemental-metadata.json`
   or `.json`, selecting exactly one same-directory filename by exact value or
   truncation prefix;
3. content-identical aliases contributing compatible metadata and albums.

Every selection has a stable rule ID. Multiple candidates, multiple
non-identical sidecars for one physical copy, or conflicting normalized values
produce errors rather than a silent winner. Album metadata is recognized only
as bounded JSON in a non-system album directory. Album titles, descriptions
and asset membership stay distinct until reconciliation.

`photoTakenTime.timestamp` is preferred over `creationTime.timestamp` and is
interpreted strictly as Unix seconds. Output is canonical RFC 3339 UTC at
second precision. Locale-dependent `formatted` strings are never parsed, so
host timezone and creation order cannot change the plan. `geoDataExif` is
preferred over `geoData`; finite in-range latitude/longitude are serialized in
canonical decimal form, while the all-zero absent sentinel remains unset.
Descriptions, titles and album names have explicit byte and control-character
limits. People, partner, face, archive/trash state and edited/original policy
remain explicitly unsupported.

### Evidence and mutation boundary

Versioned synthetic fixtures define decompressed and split-ZIP views of the
same logical corpus, including truncation, duplicate metadata, albums,
descriptions, UTC timestamps and synthetic non-personal coordinate values.
Golden and property tests require byte-identical plans across input and archive
entry order. The black-box immich-go v0.32.0 differential compares only
observable supported outcomes and classifies any unavoidable limitation or
oracle defect explicitly.

immich-rs remains wholly read-only for Takeout: no client, API key, upload,
album mutation or executor capability is reachable. The oracle runs only
against the loopback synthetic mock, and its pinned job-resume dry-run defect
remains the only accepted mutation set. A manual paired benchmark measures the
same synthetic split corpus and environment without making a generalized
performance claim.

## Consequences

Phase 3 gains a complete, bounded read-only compatibility surface without
authorizing Takeout apply. ZIP central-directory metadata is retained by the
third-party reader up to the explicit entry cap, while entry payloads and media
remain streamed. Plans produced by the prior Takeout slice are version 1
evidence and require a fresh scan to obtain normalized metadata version 2;
there is no lossy in-place migration.

Content-identical aliases intentionally collapse to one asset, which avoids
duplicate imports and permits album copies to enrich one logical operation.
Ambiguous or conflicting exports can still be inspected but carry errors that
block any future apply design.

## Verification

Unit tests cover ZIP traversal, encryption/unsupported methods, compression
ratio, CRC/read failure, duplicate/conflicting logical paths, bounded JSON,
supplemental truncation, album aliases, metadata conflicts, time/location
normalization, cancellation and deterministic order. Golden tests compare
directory and split-ZIP views. Architecture checks reject every Takeout
network or mutation dependency.

Push CI runs fixture safety, exact normalized goldens, property tests and the
pinned black-box differential. A separate manual workflow records paired raw
wall time, CPU, RSS, descriptors, I/O, operations, retries and the exact
environment/fixture manifest. Phase 3 is complete only when every declared
matrix row and both dependency and quality jobs are green on the exact SHA.
