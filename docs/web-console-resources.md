# Web Console capabilities and resource bounds

ADR-0033 requires these defaults and hard maxima. A lower operator value is
allowed; exceeding a hard maximum is a startup or request error.

## Capability matrix

| Surface | Read/plan | Dry-run | Apply | Current status at ADR acceptance |
|---|---:|---:|---:|---|
| Folder source | Planned | Planned | Planned with exact grant | Not implemented |
| Google Takeout | Planned | Planned | Planned with exact grant | Not implemented |
| Apple Photos | Planned | Planned | Planned with exact grant | Not implemented |
| Picasa | Planned | Planned | Planned with exact grant | Not implemented |
| Immich archive | Display may be added read-only | Not applicable | No web write | Not implemented |
| Immich migration | Display may be added read-only | No web apply | Unsupported, including production | Not implemented |
| Delete/replace/trash/tags/people/stacks/maintenance | No | No | Unsupported | Unsupported |
| Gallery, media preview/serving and photo management | No | No | Unsupported | Unsupported |

The first executable slice is authenticated loopback folder scan/review. Later
rows cannot be described as supported until their implementation and evidence
gate is green.

Configured state roots are required now and must resolve to private directories;
their canonical identity is revalidated at each use. A disposable server profile
uses only a literal loopback IP and has no address allowlist. A remote read
profile uses one exact HTTPS hostname and 1–32 operator-configured IPv4/IPv6 CIDR
ranges. Resolution is repeated immediately before use, accepts at most the
configured DNS-address limit, requires every answer to match the profile policy,
deduplicates and pins the complete set, preserves TLS verification of the exact
hostname, and forbids redirects. This deliberately permits explicitly
configured private LAN/VPN ranges without granting a browser general SSRF
authority. No server connection is exposed by the current slice.

The implemented `console-history-v1.sqlite3` store is separate from executor
checkpoints. It uses a transactional strict schema, full-synchronous WAL with a
bounded page count and post-write truncating checkpoints, age/count eviction,
private state permissions, startup integrity validation, and fail-closed newer
schema/corruption/size handling. Only terminal workflow/status enums, aggregate
counters and future opaque plan references are admitted by its typed write API;
the authenticated fixed-size history page does not expose internal sequences.
Dry-run receipt persistence is reserved in schema v1 but is not yet exposed or
written.

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
| Plan export response | 128 MiB | 256 MiB |
| Dry-run receipt age for confirmation | 5 min | 10 min |
| Production grant lifetime | 90 s | 120 s |
| Backup reference UTF-8 bytes | 256 | 512 |
| Configured source profiles | 32 | 64 |
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
closed without truncating an authoritative artifact.
