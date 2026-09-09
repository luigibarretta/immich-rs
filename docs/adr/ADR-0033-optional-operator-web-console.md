# ADR-0033: Optional authenticated operator Web Console

- Status: Accepted
- Date: 2026-09-09
- Owners: project maintainers
- Supersedes clauses in: ADR-0002, ADR-0004, ADR-0010, ADR-0013,
  ADR-0014, ADR-0025, ADR-0026, ADR-0027, ADR-0028 and ADR-0030

## Context

The supported CLI now has immutable folder and source-import plans, offline
dry-run, bounded executor-owned apply, durable checkpoints and production HTTPS
authorization. Operators would benefit from a review-oriented console for the
same workflows. Merely exposing the CLI or executor over HTTP would weaken the
existing filesystem, credential, confirmation and idempotency boundaries.

ADR-0002 says that the product is not a UI. ADR-0004 fixes the initial
four-crate graph. ADR-0027 requires production acknowledgements to be CLI-only,
and ADR-0028/ADR-0030 inherit that restriction for source imports. The optional
console therefore requires a recorded, narrow change before implementation.

## Decision

### Exact scope of supersession

This ADR supersedes only these prior clauses:

- ADR-0002's exclusion of every UI is narrowed to permit one optional operator
  Web Console. The product remains an import, archive and migration client, not
  an Immich server, gallery, photo manager or general backup system.
- ADR-0004's four-initial-crate graph is extended with `immich-application` and
  `immich-web`. Its library/process boundaries and inward dependency rule remain.
- ADR-0010's execution-state model is extended with a separate bounded console
  history database and ephemeral web grants. Immutable plan, executor checkpoint,
  retry, reconciliation and resume semantics remain unchanged.
- ADR-0013's local-event boundary is extended with authenticated browser views,
  aggregate web metrics and the controls in the project threat model. Its secret,
  path, metadata, TLS, least-privilege and no-telemetry defaults remain.
- ADR-0014 and ADR-0025 are extended with a separate `immich-rs-web` native
  artifact for the same five native targets. Existing CLI artifact names and
  signed-tag gates remain unchanged.
- ADR-0026 is extended with a separate hardened Web Console OCI image and
  Compose profile. The existing CLI image and offline default remain unchanged.
- ADR-0027's statement that production acknowledgements are CLI-only is narrowed:
  the CLI contract remains invocation-only and unchanged, while an authenticated
  web principal may create the single-use in-memory grant defined below.
- ADR-0028 and ADR-0030 inherit the same narrow alternative for Google Takeout
  and Apple Photos apply. Their source, plan, effect, count, backup, permission,
  convergence and unsupported-mutation clauses remain unchanged.

ADR-0031's inherited source-import authorization uses the same web grant without
changing its Picasa parsing, planning or effect surface. This ADR does not
supersede ADR-0032: production Immich-to-Immich migration remains impossible.
It does not supersede ADR-0029; the existing CLI production Compose service is
unchanged.

### Architecture and execution ownership

The workspace graph becomes:

```text
immich-cli ----\
                > immich-application -> immich-executor
immich-web ----/                        -> immich-sources
                                         -> immich-client
                                         -> immich-core

immich-executor -> immich-client, immich-sources, immich-core
immich-client, immich-sources -> immich-core
```

`immich-application` is a thin typed facade for the workflow composition already
owned by the CLI. It does not read environment variables, print, exit, parse
HTTP, persist sessions or implement durable effects. Both frontends depend on
it. The CLI retains its current observable order, including server probes, plan
bytes, stdout/stderr, exit classes and checkpoint compatibility.

`immich-executor` remains the sole owner of durable server and filesystem
effects. The web crate owns HTTP, server-rendered pages, session/CSRF policy,
configured profile lookup, jobs, SSE, history and web metrics. It never spawns
the CLI as a subprocess. Executor progress is exposed through a new versioned
application adapter; existing scan-only `ProgressEvent` meaning is not changed.

### Browser and configured-resource boundary

The browser can reference only opaque operator-configured source, state,
destination and server profile identifiers. It cannot submit an absolute path,
secret path, server URL, CA path or arbitrary configuration fragment. A profile
is resolved server-side and bound to a generation digest. At use, every local
resource is reopened and revalidated for canonical containment, regular-file or
directory type, ownership policy and symlink/reparse identity.

Secret material is read only from bounded regular files configured outside the
browser. API keys, OIDC client secrets, OIDC access/refresh tokens and production
grants never enter browser storage. Browser summaries and digests are display
data and never authorize execution.

Loopback is a transport property, not authentication. On first loopback start,
the console requires a local bootstrap secret and one-time pairing before issuing
an opaque, rotated, protected session. Restart forgets all sessions and grants.
Non-loopback startup is refused unless the complete TLS and OIDC policy below is
valid.

### HTTP, rendering and resource policy

Every request must match the configured exact Host and public origin. Forwarded
headers are rejected unless the immediate peer is in an explicit trusted-proxy
set. Every state-changing route uses a non-GET method, authenticated session,
strict exact Origin check and session-bound CSRF token.

HTML is server-rendered with contextual escaping. Raw insertion, `innerHTML`,
inline handlers and external assets are forbidden. Responses set a restrictive
CSP, frame denial, `Referrer-Policy: no-referrer`, `X-Content-Type-Options:
nosniff`, and private `no-store` caching. Every job, report, plan export, history
page, SSE stream and metrics endpoint performs authorization independently.

Headers, request bodies, accepted connections, request deadlines, sessions,
jobs, subscribers, replay, page sizes, database size and plan size have the
numeric fail-closed limits in `docs/web-console-resources.md`. A browser or SSE
disconnect does not cancel a job. Cancellation is an authenticated cooperative
operation. Shutdown stops admission, cancels bounded work, joins every owned
task and never publishes a partial plan.

Outbound Immich and OIDC requests are constructed only from profiles. HTTPS is
required except for exact disposable loopback profiles. Redirects and embedded
credentials are rejected. DNS answers, resolved-address count and destination
policy are checked at connection time so rebinding cannot widen the profile.

### Plans, history, dry-run and grants

Plans remain immutable executor/application artifacts. Plan inspection and
export require authorization and use bounded streaming. Files containing plans
or checkpoints are treated as sensitive private state.

`console-history-v1` is a distinct SQLite database, never an executor checkpoint.
It stores bounded job status, safe aggregate counters, opaque plan references and
versioned dry-run receipts. It stores no active authentication, API key, OIDC
token, production grant, raw backup reference, raw media metadata, filename,
source path, server URL or asset/user identifier. Migrations are transactional;
newer schemas, corruption or disk-full fail closed. WAL and retention are bounded.

A dry-run receipt records the exact canonical plan digest, source/configuration
digest, authenticated server/user identity, profile and credential generation,
maximum logical-effect count, and completion time. It is not a checkpoint or a
grant and cannot be converted into either implicitly.

A production grant is created only after a completed current dry-run and an
authenticated second confirmation. It is single-use and bound to all of:

1. principal, protected session and process-boot identity;
2. one admitted job identifier and idempotency key;
3. exact canonical plan and source/configuration digests;
4. authenticated server and user identity;
5. server profile and credential generation;
6. exact maximum logical-effect count;
7. SHA-256 of the bounded operator-supplied backup reference;
8. the exact completed dry-run receipt; and
9. a short monotonic deadline.

Grant consumption and job admission are one atomic operation. The raw backup
reference and grant are never persisted, logged or reconstructed. Repeating the
same admission request returns the already admitted job or refuses; it never
runs twice. Logout, restart or pre-admission cancellation invalidates an unused
grant. An admitted run owns a run-scoped capability and writes the existing
executor checkpoint. Resume always requires a new dry-run and confirmation.
Maximum mutations means the plan's maximum logical effects, not an HTTP request
ceiling.

### OIDC and LAN mode

LAN mode uses authorization-code flow with PKCE. State and nonce are bounded,
single-use and session-initiation bound. The server performs the code exchange
and validates exact issuer, client, redirect URI, signature, allowed algorithm,
audience and expiry. Only an issuer-plus-subject allowlist or explicit mapped
operator role can create a session.

Non-loopback mode requires TLS, secure cookies, exact public origin and a complete
trusted-proxy policy when a proxy is used. Login rotates session identifiers and
enforces idle and absolute expiry. Provider metadata and JWKS fetches are bounded,
cached for a bounded interval and fail closed during invalid rotation or outage.
Tests use only a disposable IdP and TLS environment; live Authentik is excluded.

### Packaging and observability

The native binary and artifact are named `immich-rs-web` and are built for the
same five ADR-0025 targets: Linux x86-64/arm64, macOS x86-64/arm64 and Windows
x86-64. The Web Console has a separate versioned OCI image, per-platform SPDX
SBOM and provenance. The image runs non-root with a read-only root filesystem,
drops all capabilities, mounts sources and secrets read-only, and has only
bounded state/staging writes. Default publication is loopback-only. `latest` is
not a supported reference.

Metrics are aggregate and low-cardinality. Labels never include filenames,
paths, hashes, origins, jobs, principals, users, sessions or metadata. The
metrics listener follows the same authentication and network policy as other
console endpoints.

## Consequences

The console provides a review and execution surface without becoming an
alternative engine. Authentication and operational configuration are mandatory
even for local use. Production apply requires more interaction than read-only
planning, while CLI automation and existing artifacts remain compatible.

SQLite history, OIDC, TLS and browser testing add dependencies and release work.
They are admitted only when compatible with Rust 1.88, AGPL-3.0-only distribution,
rustls and the repository audit policy.

## Verification

Architecture checks enforce the graph and reject CLI subprocess execution or
web-owned durable effects. Golden tests prove unchanged CLI streams, exits, plan
bytes and checkpoints. Compile-fail capability tests retain existing token and
dependency protections while proving dry-run and read-only code cannot reach a
mutation constructor.

Browser/security tests cover bootstrap, sessions, fixation, CSRF, Host, Origin,
proxy spoofing, escaping, CSP, path swaps, profile drift, bounded admission,
SSE isolation/replay/disconnect, cancellation and clean shutdown. SQLite tests
cover migration, newer schema, corruption, disk-full, retention and restart.
Grant tests cover races, replay, expiry, logout, restart and every binding
mismatch. Disposable loopback/HTTPS Immich, IdP and container gates use only
synthetic data and prove cleanup. Production endpoints, credentials, media,
live Authentik and the homelab immich-go smoke are never test targets.
