# ADR-0027: Production transport and operator authorization

- Status: Accepted
- Date: 2026-08-23
- Owners: project maintainers
- Supersedes: ADR-0020 and ADR-0024

## Context

ADR-0020 proved idempotent folder upload through an isolated loopback forwarder,
and ADR-0024 proved a read-only archive against a disposable loopback server.
Those boundaries prevented premature production access, but they also make the
current commands unusable against a normal remote Immich deployment. Removing
the loopback check alone would turn a test capability into an accidental
production mutation path without an operator acknowledgement, backup evidence
or an exact mutation budget.

The archive contract from ADR-0024 remains read-only and the upload plan,
streaming, retry, checkpoint and duplicate guarantees from ADR-0019/ADR-0020
remain required. This decision replaces only their transport and authorization
boundary while restating the retained safety properties.

## Decision

### Transport modes

The client has two explicit endpoint modes. Disposable mode accepts only a
literal loopback origin and permits HTTP for isolated integration tests.
Production mode accepts only a remote HTTPS origin with normal certificate and
hostname verification. Redirects, embedded credentials, URL paths, query
strings, fragments and insecure TLS overrides remain rejected.

Production activation is command-line only. It cannot be stored in TOML,
environment defaults, Compose configuration or a checkpoint. Merely supplying
a remote server or API key never enables production access.

Read-only archive planning and apply require an explicit
`--authorize-production-read` acknowledgement for a remote origin. That grant
can construct only the existing archive/read capability and cannot be upgraded
to upload, metadata, album, delete or replace access.

### Mutation authorization

Remote upload apply additionally requires all of these command-line values:

1. `--authorize-production-write`;
2. `--confirm-plan-sha256` equal to the compact immutable upload-plan digest;
3. `--expected-operations` equal to the plan operation count;
4. `--backup-reference` containing a bounded operator-supplied reference to a
   verified backup or restore point.

The backup reference is hashed before it enters a report or checkpoint. The
raw value, server origin and API key are never serialized. Dry-run ignores all
production flags, loads no API key and constructs no network client. A changed
plan, source, server identity, operation count, configuration or checkpoint
fails before the first mutation.

Version 0.1 production writes remain limited to the existing immutable folder
upload operation set: standalone image/video, one XMP sidecar and unambiguous
image/MOV Live Photos. Delete, replace, independent metadata mutation, tags,
stacks, people and maintenance commands remain unavailable. Google Takeout and
Apple Photos apply require separate source-specific ADRs and gates.

### Evidence boundary

Automated tests use only synthetic data and disposable Immich instances. A
production-like gate places a disposable supported Immich version behind a
dedicated HTTPS origin, uses a least-privilege synthetic owner/key, performs
plan, dry-run, authorized apply, interruption/resume and duplicate convergence,
then removes the exact key, owner, containers, volumes, network, certificates
and workspace.

No CI job receives production credentials or a production hostname. A selected
real import remains a separate maintainer-authorized cutover step under
ADR-0015 with a verified backup, bounded source, expected mutations,
postconditions and rollback. The periodic immich-go homelab smoke is unchanged.

## Consequences

Users can operate the proven folder uploader and read-only archive against a
normal HTTPS Immich deployment without weakening disposable tests. Production
writes are intentionally less convenient because every invocation must bind an
operator acknowledgement to the exact immutable plan and mutation count.

The CLI and core gain production-authorization types and stable validation
errors. Configuration files remain safe to reuse because they cannot silently
enable production. Broader import semantics still arrive as separate verticals
instead of making the first remote gate authorize untested metadata writes.

## Verification

Unit tests cover loopback/remote mode separation, HTTPS enforcement, redirects,
credential-bearing URLs, missing or mismatched acknowledgements, backup
reference redaction and dry-run dependency isolation. Mock tests prove that a
remote read grant cannot reach a mutating method and that write authorization
is bound to the exact plan, server identity and operation count.

The disposable HTTPS gate covers valid certificates, untrusted certificates,
authentication, incompatible versions, 429, 5xx, disconnect, commit with lost
response, cancellation, resume and a fresh-checkpoint duplicate run. CI checks
the dependency graph and scans stdout, stderr, reports and artifacts for the
secret and backup canaries. Production mode is not documented as supported
until this evidence is committed and green on the exact implementation SHA.
