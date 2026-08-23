# ADR-0028: Google Takeout import

- Status: Accepted
- Date: 2026-08-23
- Owners: project maintainers
- Supersedes: ADR-0022

## Context

ADR-0022 completed deterministic read-only planning for decompressed and split
ZIP Google Takeout exports, but explicitly prevented those plans from entering
the executor. ADR-0027 subsequently proved remote HTTPS folder upload with an
exact production authorization, durable checkpoint and bounded retry contract.
A Takeout import must preserve its resolved timestamps, descriptions,
coordinates and albums without weakening either boundary.

Immich v3.1.0's official OpenAPI exposes bounded asset upload, asset metadata
update, album list/create and album membership endpoints. These are additional
mutations, so treating a Takeout plan as a folder plan would understate the
operator's mutation budget and would make a lost response during album creation
ambiguous.

## Decision

### Plan and input contract

`plan upload google-takeout` accepts the same one decompressed directory or one
to 64 independent ZIP inputs as `plan google-takeout`. It emits
`upload-plan-v2`, retaining the exact `normalized-plan-v2` digest, server and
configuration binding, media/XMP/live-photo operations, normalized metadata,
sorted album names and a maximum mutation budget.

`apply upload` rescans the exact input set selected by repeated `--input`
options or `IMMICH_RS_INPUTS_JSON`; the existing `--source` remains the
single-directory shorthand. Mixing directories and archives, changing an
input, changing reconciliation options or presenting a v2 plan to the folder
scanner fails before a network capability is constructed.

ZIPs are never globally extracted. Verification reopens the selected archive
entry by its bounded index and portable path. Immediately before an upload,
each worker streams at most one media and its optional XMP into an exact
checkpoint-adjacent staging file, verifies length and SHA-256, uploads from
that file, then removes it. The staging directory is symlink-safe, plan-bound
and removed on normal completion; a later invocation may remove only its exact
stale plan-bound files. Memory, descriptors, archive count, entry size,
compression ratio, concurrency and staging bytes retain explicit limits.

### Mutation and convergence contract

An import operation first converges the asset upload by checksum. It then
applies only non-empty normalized fields with Immich's v3.1 asset-update API:
capture instant, description and paired latitude/longitude. The update is an
idempotent exact assignment and is checkpointed separately from upload. A
duplicate asset is updated only when the immutable plan explicitly contains
normalized metadata and the operator authorized that mutation.

Albums are processed after every member asset has a durable server ID. Exact
album names are listed before creation. Zero matches permits create, one match
is reused and more than one match fails closed. A lost create response is
reconciled by listing again; no blind second create is allowed. Membership is
one deterministic request per album, treats already-present members as
converged and is checkpointed separately. Existing albums are never renamed,
deleted or otherwise modified.

The plan summary records asset uploads, metadata updates, distinct album
creates and album-membership requests. `--expected-operations` binds to their
sum, which is the maximum possible server mutation count. The production
confirmation from ADR-0027 remains CLI-only and binds the complete v2 plan,
source, server and verified-backup reference. Folder `upload-plan-v1` and its
operation budget remain byte-compatible.

The API key used for production must have only the permissions required by the
selected plan: asset upload/read/update and, when albums exist, album
read/create plus album-asset create. The client exposes a distinct import
capability; folder upload, archive read and source adapters cannot reach
metadata or album methods. Delete, replace, trash, tag, people, stack and
independent maintenance mutations remain absent.

### Evidence boundary

Tests use generated Takeout directory and split-ZIP views, the loopback mock
and a disposable Immich v3.1.0 HTTPS origin. The black-box immich-go v0.32.0
oracle is used only for observable differential behavior; no Go source or
derived implementation detail is consulted.

The gate proves byte-identical directory/ZIP import plans, offline dry-run,
metadata and album postconditions, duplicate convergence, lost responses at
upload/metadata/album stages, cancellation at every boundary, resume from each
durable effect and exact cleanup. A paired comparison uses the same synthetic
corpus/server/environment and publishes raw results before any performance
claim.

## Consequences

Split archives trade bounded temporary disk I/O for retryable streaming HTTP
bodies; they do not require enough RAM or free disk for the complete export.
Imports may perform more server writes than assets, but the maximum is visible
before authorization. Album-name collisions intentionally require operator
cleanup or a future explicit conflict policy.

Takeout planning remains useful without credentials and cannot construct a
client. Applying descriptions, capture instants, locations or albums becomes
possible only through the new plan version, import capability and checkpointed
production confirmation.

## Verification

Unit and property tests cover v1/v2 schema separation, mutation-budget
calculation, input permutation, directory/ZIP equivalence, archive staging
bounds, source changes and deterministic metadata/album effects. Mock tests
assert exact request bodies, permissions, retry classification, lost-response
reconciliation and that dry-run issues zero requests.

The disposable gate records only synthetic aggregate evidence, exact image and
binary digests, report counters, postconditions and zero-resource cleanup. Push
CI validates the evidence and dependency boundary. Production Takeout apply is
not documented as supported until the exact implementation SHA, disposable
gate and paired benchmark are committed and green.

The API contract was verified against the official Immich v3.1.0 OpenAPI:
<https://github.com/immich-app/immich/blob/v3.1.0/open-api/immich-openapi-specs.json>.
