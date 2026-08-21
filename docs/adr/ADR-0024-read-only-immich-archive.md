# ADR-0024: Read-only Immich archive

- Status: Accepted
- Date: 2026-08-21
- Owners: project maintainers

## Context

Phase 5 requires an archive sourced from Immich while the project is not
authorized to mutate production. A local archive writes user-selected local
storage but must never acquire a server mutation capability. Large originals
cannot be buffered in memory, and a retry or lost local process must converge
without overwriting a different existing file.

Immich asset identifiers, names and checksums are server data. They may appear
in an explicitly requested local manifest, but default logs and reports must
remain privacy-aware. The first implementation is proven against the synthetic
mock and an isolated disposable Immich only.

## Decision

### Immutable manifest

`plan archive immich` authenticates with the read-only client, negotiates the
exact supported server version and enumerates assets through bounded paginated
searches. It accepts an explicit visibility selection (`timeline`, `archive`,
`hidden` or `all`) and an explicit trash inclusion flag. The default is
non-trashed timeline assets. Pages are capped at 1,000 items and the total asset
count is bounded by caller configuration.

The command emits `archive-manifest-v1`: server compatibility binding, stable
configuration digest, sorted assets, bounded original filename, media type,
byte length and Immich's SHA-1 checksum. Duplicate asset IDs, duplicate target
paths, malformed checksums, missing sizes, pagination drift and source changes
fail closed. Target paths are `assets/<opaque-id>/<safe-original-filename>`;
path separators, dot components, control characters and platform-reserved
names are rejected rather than rewritten.

### Local archive apply

`apply archive` requires the immutable manifest, destination, loopback server
and API key. The manifest is validated before network access and is rebound to
the freshly negotiated server identity. The read client exposes only GET/POST
search and GET original-download methods; it cannot be converted to an upload
client in the archive execution path.

Each original streams through a bounded buffer to a sibling `.part` file while
SHA-1 and byte length are calculated. A matching existing final file is
reported as already complete. A different existing file is never overwritten.
After checksum and size match, the part is flushed and atomically renamed.
Cancellation or failure removes only the exact part created by that operation.
The filesystem plus immutable manifest form the resumable checkpoint: a rerun
revalidates completed files and downloads only missing assets.

Execution is initially sequential. This is a bounded concurrency setting, not
an implicit performance claim. Reports use `archive-apply-report-v1` and expose
only counts, bytes, retries, manifest digest and stable exit classes; they do
not include server URLs, API keys, original names or destination paths.

### Authorization boundary

Phase 5 continues to require a literal loopback or localhost endpoint. HTTPS
does not waive this restriction. Synthetic mock and disposable evidence may
exercise the feature; production, a production forwarder and production
credentials remain unauthorized. Replacement, delete, metadata mutation and
stack commands are unavailable and require a superseding ADR plus the staged
mutation gates of ADR-0015.

## Consequences

The first archive is a faithful original-byte backup with deterministic local
layout and verified convergence. It does not recreate Immich thumbnails,
transcoded video, database state, people, albums, tags, stacks or sharing
relationships. Those relationships can be added to a later read-only manifest
schema without weakening the byte archive.

Server-side POST search requests are classified read-only by path and contract;
the mock rejects any unexpected mutation. The output destination must be a
real directory and symlink components are rejected to prevent escape.

## Verification

Core tests cover manifest ordering, identities, filenames, target collisions
and report invariants. Client/mock tests cover pagination, authentication,
version mismatch, timeout, 429, 5xx, disconnect, bounded bodies and streaming
download cancellation. Executor tests cover checksum success, existing-file
resume, conflict refusal, source drift, atomic rename and exact part cleanup.

Push CI proves no archive dependency can construct an upload capability and
that all test requests are non-mutating. A disposable Immich canary uploads
only synthetic assets through the already authorized Phase 2 path, archives
them with the Phase 5 read-only path, verifies byte identity and removes every
labelled container, volume, network and downloaded image afterward.
