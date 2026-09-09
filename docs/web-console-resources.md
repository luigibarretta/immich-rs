# Web Console capabilities and resource bounds

ADR-0033 requires these defaults and hard maxima. A lower operator value is
allowed; exceeding a hard maximum is a startup or request error.

## Capability matrix

| Surface | Read/plan | Dry-run | Apply | Current implemented status |
|---|---:|---:|---:|---|
| Folder source | Implemented | Implemented | Implemented with exact grant | Disposable loopback apply through the executor |
| Google Takeout | Implemented | Implemented | Implemented with exact grant | Synthetic split-ZIP apply and convergence evidence |
| Apple Photos | Implemented | Implemented | Implemented with exact grant | Synthetic directory apply; private shadow pending |
| Picasa | Implemented | Implemented | Implemented with exact grant | Synthetic directory apply; private shadow pending |
| Immich archive | Display may be added read-only | Not applicable | No web write | Not implemented |
| Immich migration | Display may be added read-only | No web apply | Unsupported, including production | Not implemented |
| Delete/replace/trash/tags/people/stacks/maintenance | No | No | Unsupported | Unsupported |
| Gallery, media preview/serving and photo management | No | No | Unsupported | Unsupported |

Source-import apply uses the same grant and admission boundary as folder apply;
it does not widen the adapter-specific immutable plans or effect budgets.

Configured state roots are required now and must resolve to private directories;
their canonical identity is revalidated at each use. A disposable server profile
uses only a literal loopback IP and has no address allowlist. A remote read
profile uses one exact HTTPS hostname and 1–32 operator-configured IPv4/IPv6 CIDR
ranges. Resolution is repeated immediately before use, accepts at most the
configured DNS-address limit, requires every answer to match the profile policy,
deduplicates and pins the complete set, preserves TLS verification of the exact
hostname, and forbids redirects. This deliberately permits explicitly
configured private LAN/VPN ranges without granting a browser general SSRF
authority. Only an authenticated server-bound planning job can construct this
read capability; source-only scan and offline dry-run paths cannot construct it.

The implemented `console-history-v1.sqlite3` store is separate from executor
checkpoints. It uses a transactional strict schema, full-synchronous WAL with a
bounded page count and post-write truncating checkpoints, age/count eviction,
private state permissions, startup integrity validation, and fail-closed newer
schema/corruption/size handling. Only terminal workflow/status enums, aggregate
counters and opaque plan references are admitted by its typed write API; the
authenticated fixed-size history page does not expose internal sequences. Its
transactional schema v2 migration expands the workflow enum without renaming
the distinct `console-history-v1.sqlite3` format family. A completed dry-run
atomically writes its terminal row and a random opaque receipt binding
the canonical plan, combined source-profile/configuration identity,
authenticated server identity, server profile and credential generation, exact
logical-effect maximum and completion time. Receipt inspection is authenticated
and informational; a receipt is neither a checkpoint nor a grant.

Completed folder and source-import upload plans are pretty-JSON application
artifacts stored in private create-new files under random 128-bit opaque
references. A separate
private binding contains only the plan digest, opaque source/server profile IDs,
their generation digests, credential generation, authenticated server identity
digest and exact logical-effect count. Publication uses bounded streaming
serialization, synchronized files and atomic same-directory renames. Every
inspection and streaming export revalidates the state-root identity, file type,
permissions, size, identity, plan schema and all binding digests. It exposes no
server origin or source path in the HTML summary.

Dry-run reopens that private artifact, revalidates source/state/server bindings
before and after executor-owned offline verification, and uses a fresh absent
checkpoint pathname. Any unexpected checkpoint is removed and fails the job.
The capability canary test stops the disposable server and removes its key file
after the two planning probes; dry-run still completes with the request counter
unchanged, proving neither the secret loader nor client construction path was
reached. Source-content drift and server-profile binding drift fail without a
network request or receipt.

Folder and supported source-import apply require an authenticated second confirmation that retypes the
exact canonical plan digest and maximum logical-effect count. The in-memory
grant is bound to the session, receipt, plan, source/configuration, server/user
identity, profile and credential generations, count and monotonic deadline.
Production profiles additionally bind the SHA-256 of a raw backup reference
that is never persisted or displayed. Grant consumption and bounded job
admission share one lock-protected transition; a duplicate admission returns
the same job. A bounded process-local spent-receipt set prevents a receipt from
authorizing resume twice. The executor-owned worker verifies the source and any
existing private checkpoint offline before loading a credential or probing the
server. Checkpoint creation marks the start of a run; after cancellation or
restart, only a later completed dry-run receipt may authorize checkpoint resume.
Tests cover replay races, expiry, logout, restart, drift, cancellation, resume,
repeated cancellation and bounded before/after-commit recovery against a
synthetic loopback server. Google Takeout, Apple Photos and Picasa scan, plan,
offline dry-run and apply retain the established `normalized-plan-v2/v3/v4`
and `upload-plan-v2` contracts and operator-configured archive, album and
executor budgets. The import gate covers a real synthetic ZIP, directory
sources, metadata/album effects, single-use admission, source drift and
fresh-plan duplicate convergence through executor-owned `checkpoint-v2`.
Synthetic coverage does not satisfy the pending external private Apple/Picasa
shadow gates.

Direct LAN mode terminates TLS 1.3 in `immich-web` and requires an exact HTTPS
public origin plus operator-owned certificate/private-key files. It uses OIDC
authorization code with PKCE, a single-use in-memory state/nonce/verifier, fresh
bounded discovery and JWKS fetches, server-side token exchange and EdDSA-only ID
token verification. The exact issuer hostname is resolved to at most the DNS
address limit; every deduplicated answer must fall within one of 1–32 explicit
operator CIDRs and is pinned without disabling TLS hostname verification.
Subject allowlisting and a required `roles` claim are mutually exclusive.
Sessions are rotated on login, use Secure/HttpOnly/SameSite=Strict cookies and
retain the existing idle/absolute limits. Forwarded headers and trusted
reverse-proxy termination are unsupported and rejected.

## Aggregate metrics

Authenticated `GET /metrics` returns a fixed Prometheus text payload on the
same configured console listener. Exact Host and session checks apply in
loopback and direct-TLS LAN modes; LAN authentication is still OIDC, TLS is
still mandatory, and forwarded headers remain rejected. There is no separate
metrics listener, bearer token or browser-configurable network target.

The complete metric allowlist is:

- `immich_rs_web_sessions_active`;
- `immich_rs_web_jobs_queued`;
- `immich_rs_web_jobs_running`;
- `immich_rs_web_jobs_completed`;
- `immich_rs_web_jobs_failed`;
- `immich_rs_web_jobs_cancelled`;
- `immich_rs_web_jobs_retained`;
- `immich_rs_web_sse_subscribers`;
- `immich_rs_web_history_rows`.

All are instantaneous aggregate gauges with no labels. They are not durable
counters. Filenames, paths, plan/profile/source hashes, origins, job, user or
session identifiers, and media metadata are absent by construction and by
forbidden-value tests. Session/job lock failure or history-store failure returns
503 instead of partial metrics. A logged-out or expired session receives 401.
Automated bearer-token scraping is unsupported in this version; an operator
must deliberately supply a current protected console session under the same
network policy.

## Numeric bounds

| Resource | Default | Hard maximum |
|---|---:|---:|
| Request header bytes | 16 KiB | 32 KiB |
| State-changing request body | 16 KiB | 64 KiB |
| Accepted connections | 16 | 32 |
| Header/read deadline | 5 s | 10 s |
| Non-streaming response deadline | 15 s | 30 s |
| Sessions per process | 8 | 16 |
| Session idle lifetime | 30 min | 60 min |
| Session absolute lifetime | 8 h | 12 h |
| Bootstrap lifetime | 10 min | 15 min |
| Pairing failures per source in 5 min | 5 | 5 |
| Concurrent running jobs | 1 | 4 |
| Queued jobs | 4 | 8 |
| Retained in-memory job records | 128 | 256 |
| SSE subscribers per session | 4 | 8 |
| SSE subscribers per process | 16 | 32 |
| SSE replay events per job | 128 | 256 |
| SSE heartbeat | 15 s | 30 s |
| History page rows | 50 | 100 |
| History retained rows | 5,000 | 10,000 |
| History retention | 30 days | 90 days |
| History database plus WAL | 32 MiB | 64 MiB |
| Immutable plan file | 128 MiB | 256 MiB |
| Aggregate immutable plan store | 512 MiB | 4 GiB |
| Plan export response | 128 MiB | 256 MiB |
| Dry-run receipt age for confirmation | 5 min | 10 min |
| Production grant lifetime | 90 s | 120 s |
| Backup reference UTF-8 bytes | 256 | 256 |
| Configured source profiles | 32 | 64 |
| Relative inputs per import source profile | adapter default | 64 |
| Configured state/destination profiles | 16 | 32 |
| Configured server profiles | 16 | 32 |
| Address CIDR ranges per server profile | 32 | 32 |
| DNS addresses accepted per host lookup | 4 | 8 |
| OIDC discovery document | 64 KiB | 128 KiB |
| OIDC JWKS document | 256 KiB | 512 KiB |
| OIDC signing keys | 8 | 16 |
| OIDC state/nonce lifetime | 3 min | 5 min |
| OIDC token/claims response | 32 KiB | 64 KiB |

Existing scanner, archive, media, staging, retry, concurrency and mutation-count
limits remain authoritative below this web layer. The HTTP request limit is
never used as a substitute for the immutable plan's maximum logical effects.

History eviction removes oldest terminal rows in bounded transactions. Active
jobs are never evicted. Reaching a disk, plan, job or database hard bound fails
closed without truncating an authoritative artifact. Immutable plans and apply
checkpoints are not automatically pruned in this version; the safe rollover
procedure is documented in the
[operator and recovery guide](web-console-operations.md).
