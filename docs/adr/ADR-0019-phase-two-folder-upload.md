# ADR-0019: Phase-two folder upload execution

- Status: Superseded
- Superseded by: ADR-0020
- Date: 2026-08-15
- Owners: project maintainers

## Context

Phase 1 can describe a folder without any server mutation capability. Phase 2
must upload that immutable source safely while preserving bounded resources,
explicit plan/apply separation, crash recovery and the production isolation
required by ADR-0015. An HTTP success alone cannot prove idempotency: a response
may be lost after Immich committed an asset, or the checkpoint write may fail
after the server response arrived.

## Decision

### Capability and command boundary

Phase 2 adds a fifth workspace crate, `immich-executor`, because durable apply
orchestration is a real dependency boundary. It depends on core, sources and
client; sources and core remain unable to reach HTTP. The client exposes
separate read-only and upload capability types. Constructing an upload
capability requires a successful authenticated compatibility probe.

The public workflow is explicit:

1. `plan folder` remains the server-independent `normalized-plan-v1` command;
2. `plan upload folder` scans the source, performs read-only server negotiation
   and emits immutable `upload-plan-v1` JSON;
3. `apply upload` consumes that plan, source root and checkpoint path;
4. `apply upload --dry-run` validates the source and checkpoint without loading
   an API key or constructing an upload capability.

Phase 2 permits only loopback Immich origins. HTTP is accepted only for
`127.0.0.1`, `[::1]` or `localhost`; general non-loopback use remains disabled.
This is a disposable-server gate, not production authorization. API keys come
from the dedicated secret environment variable and never from an argument,
plan, journal, event, error or debug representation.

### Upload plan and source verification

`upload-plan-v1` records the normalized-plan digest, source fingerprint,
stable operations, captured filesystem timestamps, configuration digest,
authenticated server-user identity digest and exact compatible server version.
It excludes the API key, absolute paths and raw endpoint identity. The first
compatibility range is Immich 3.1.x; every other minor or major fails closed.

Apply rescans the source through the Phase 1 adapter and requires the compact
normalized-plan digest to match. Each operation is reopened through its
resolved native path, streamed through bounded SHA-256 and SHA-1 verification,
and rejected if length, content or captured timestamps changed. This deliberate
preflight read permits Immich's SHA-1 duplicate contract without weakening the
SHA-256 source identity. Upload then reopens the file and streams multipart
bytes; media and sidecars are never collected in full.

Standalone image/video assets, image/MOV live-photo pairs and at most one XMP
sidecar per asset are supported. Live-photo video operations complete before
their image dependency. Generic JSON metadata and ambiguous/multiple sidecars
fail explicitly until their Phase 3 reconciliation contract exists. Phase 2
does not expose delete, replace or independent metadata mutation.

### Duplicate, retry and checkpoint contract

Before every upload attempt, the executor calls Immich's bulk upload check with
the operation ID and SHA-1. It also sends the checksum on multipart upload. A
rejected duplicate with an existing asset ID is a successful converged outcome,
not a retry or failure. A lost response is reconciled by repeating the duplicate
check before any repeated body upload.

The local SQLite checkpoint uses schema `checkpoint-v1`, WAL mode, full
synchronous durability and append-only operation events. Immutable metadata
binds the journal to plan, source, configuration, server version and the hashed
server/user identity. Resume refuses any mismatch. Stable operation IDs, never
queue positions, identify events. Reports distinguish created, duplicate,
resumed, retried, failed, indeterminate and cancelled work without exposing
paths, hashes or server asset IDs.

Concurrency defaults to one and is bounded to at most eight operation groups.
Every task is owned by a structured scope; queues and in-flight media streams
have explicit limits. Retry budgets exist per operation and per run. Only
transport disconnects/timeouts and HTTP 408, 425, 429 or 5xx responses retry.
Authentication, validation, compatibility and invariant failures never retry.
Backoff is capped exponential delay plus deterministic operation-derived
jitter; `Retry-After` is honored only within the configured cap. Cancellation
stops new work, aborts request waits and leaves a valid checkpoint.

### Disposable integration gate

Integration evidence uses exact Immich v3.1.0 server and official dependency
image digests. The harness creates uniquely named and labelled containers,
volumes and one private Docker network. Only the Immich HTTP port is published,
and only to an ephemeral `127.0.0.1` port. No production network, route, mount,
credential, media or endpoint is available to the disposable stack.

Fixture material, account identity and credentials are generated synthetic
values. A cleanup trap removes the exact containers, volumes, network,
workspace and any test-only images after success, failure or interruption.
Evidence is valid only if post-cleanup inspection finds zero resources bearing
the run label. The gate proves a first apply, a second idempotent apply, fault
recovery after a lost response and one server asset per source operation.

## Consequences

Phase 2 reads each new media file once for verification and once for upload,
trading extra source I/O for deterministic source-change and duplicate safety.
SQLite and upload-plan schemas become migration surfaces. The loopback-only
restriction prevents premature production use; a later production phase must
supersede that restriction with its own security evidence.

Live-photo dependencies reduce available parallelism for those pairs. Generic
JSON metadata remains planned but unapplied instead of being silently dropped.
Durable journals add filesystem I/O but make uncertain server outcomes
recoverable and auditable.

## Verification

Unit and contract tests prove secret redaction, exact version refusal, bounded
response and multipart streaming, retry classification, retry caps and dry-run
capability separation. Fault tests cover disconnect, 429, 5xx, commit with lost
response, cancellation and journal mismatch/corruption.

The disposable test records image digests, fixture and plan digests, commands,
operation counts, first/second-run reports, server-observed asset counts and
the zero-resource cleanup check. CI verifies the evidence schema and runs all
mock-based tests on every push; the full disposable and paired benchmark gate
remain explicit manual workflows.
