# Phase 5 read-only archive compatibility matrix

Phase 5 archives original bytes from a loopback-only Immich endpoint into a
caller-selected local directory. The server path has only authenticated probe,
read-only metadata search, asset lookup and original-download capabilities.
No archive module can construct upload, replace, delete or metadata-mutation
capabilities.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Standalone original inventory | Parity | Both tools archive the same four assets from one owner on the same disposable Immich v3.1.0 server. |
| Original bytes | Exact parity | Both outputs match the same 587,015-byte source SHA-256 multiset. |
| Pagination | immich-rs contract | Authoritative `nextPage` tokens are followed with bounded loop detection; deprecated `total` is not trusted. |
| Visibility and trash | immich-rs contract | Timeline is the default; archive, hidden, all and explicit trash inclusion are deterministic. |
| Linked Live Photo original | immich-rs contract | The disposable canary follows `livePhotoVideoId` and archives both original members. |
| Resume | Idempotent | The first apply downloads four originals; the second verifies four completed files and downloads zero. |
| Existing-file conflict | Fail closed | A mismatching final file is not overwritten and returns stable exit class 9. |
| Retry and disconnect | Bounded recovery | Mock 429, 5xx and disconnect faults are capped; partial files are removed. |
| Cancellation | Clean convergence | Cancellation removes the exact `.part`; an unchanged rerun completes normally. |
| Server mutation | Impossible | Mock and dependency checks prove that archive planning/apply perform no mutating request. |

`archive-manifest-v1` binds the negotiated server version, configuration,
sorted asset IDs, safe original names, exact byte lengths and server SHA-1
checksums. Duplicate IDs or targets, malformed facts, pagination drift and
source changes fail closed. Apply streams each original into a sibling `.part`
file, verifies length and SHA-1, flushes it and atomically renames it. Symlink
components and path escape are rejected.

The paired benchmark uses concurrency one, one warm cache, alternating order,
two warmups and six retained pairs. Startup and post-run byte verification are
excluded equally. immich-rs median wall time is 40.964 ms versus 93.808 ms for
immich-go v0.32.0, 56.3% lower on this exact four-asset synthetic archive. The
immich-rs maximum (55.234 ms) is below the immich-go minimum (77.248 ms). This
is not a large-library, cold-storage or production claim.

The real disposable canary is bound to implementation SHA
`d3039c908d22249eeb6eb7c4c96500d80e1d6e04`; all labelled containers,
volumes, networks and downloaded images were removed. The committed raw
[benchmark](../../benchmarks/evidence/phase5-2026-08-22.json) and
[disposable evidence](../evidence/phase5-disposable-archive-2026-08-22.json)
are recalculated and fail-closed by push CI.
