# immich-rs

`immich-rs` is an independent Rust implementation of a high-integrity import,
archive and migration client for [Immich](https://immich.app/).

## Performance at a glance

Six reproducible comparisons currently satisfy ADR-0012's threshold for a
scoped performance statement against the pinned immich-go v0.32.0 oracle:

| Lower is better | immich-rs | immich-go | Difference |
|---|---:|---:|---:|
| Folder scan/plan median | 49.371 ms | 57.624 ms | immich-rs 14.3% lower |
| Folder scan/plan p95 | 51.393 ms | 58.321 ms | immich-rs 11.9% lower |
| Read-only archive median | 40.964 ms | 93.808 ms | immich-rs 56.3% lower |
| Read-only archive p95 | 55.234 ms | 101.784 ms | immich-rs 45.7% lower |
| Archive median peak RSS | 6.47 MiB | 14.92 MiB | immich-rs 56.6% lower |
| Takeout plan + import median | 1.162 s | 39.048 s | immich-rs 97.0% lower |
| Takeout plan + import p95 | 1.671 s | 39.162 s | immich-rs 95.7% lower |
| Takeout import median peak RSS | 10.55 MiB | 16.45 MiB | immich-rs 35.9% lower |
| Apple plan + import median | 0.729 s | 33.788 s | immich-rs 97.8% lower |
| Apple plan + import p95 | 0.770 s | 93.021 s | immich-rs 99.2% lower |
| Apple import median peak RSS | 10.58 MiB | 15.77 MiB | immich-rs 32.9% lower |
| Picasa plan + import median | 0.725 s | 87.064 s | immich-rs 99.2% lower |
| Picasa plan + import p95 | 0.783 s | 99.286 s | immich-rs 99.2% lower |
| Picasa import median peak RSS | 10.43 MiB | 15.11 MiB | immich-rs 31.0% lower |
| Immich migration median | 1.395 s | 124.048 s | immich-rs 98.9% lower |
| Immich migration p95 | 1.641 s | 128.529 s | immich-rs 98.7% lower |
| Migration median peak RSS | 12.66 MiB | 16.30 MiB | immich-rs 22.4% lower |

The folder comparison uses the same page-cached deterministic 64 MiB,
eight-asset synthetic corpus. The archive comparison uses the same owner and
four standalone originals totalling 587,015 bytes on one disposable Immich
v3.1.0 server with a warm cache. The Takeout comparison measures complete
planning and import of the same eight conventional-sidecar assets totalling
64 MiB into a fresh isolated owner per tool on one disposable HTTPS server.
All use concurrency one, alternating order, two warmups and six retained
pairs. Every retained immich-rs wall-time sample is below every corresponding
immich-go sample.

The Apple Photos and Picasa comparisons measure complete immutable planning
plus apply into a fresh owner per tool on one server. The Immich migration
comparison measures complete inventory, planning and migration between fresh
owners on the same two servers. All three use the same 64 MiB/eight-asset
synthetic compatibility intersection, concurrency one, alternating order, two
warmups and six retained pairs. Setup, seeding and postcondition probes are
outside the measured interval.

These are small synthetic CPU/network-loopback results. They do not measure a
large library, cold storage, WAN or production latency. See the
[Phase 1 raw report](benchmarks/evidence/phase1-2026-08-21.json),
[Phase 5 raw report](benchmarks/evidence/phase5-2026-08-22.json),
[Phase 8 raw report](benchmarks/evidence/phase8-2026-08-24.json),
[Apple](benchmarks/evidence/phase9-source-import-benchmark-2026-08-24.json),
[Picasa](benchmarks/evidence/phase10-source-import-benchmark-2026-08-24.json),
[migration](benchmarks/evidence/phase11-real-2026-08-24.json) and the full
[benchmark methodology](benchmarks/README.md). No broader speed claim is
supported by these measurements.

Phase 0 and Phase 1 are complete for implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`: push CI
[run 5165](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5165)
and manual benchmark
[run 5170](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5170)
are green. The first Phase 2 vertical adds an explicit, idempotent folder-upload
workflow restricted to disposable loopback Immich instances. Delete, replace
and independent metadata mutation remain unavailable.

Phase 3 completed the read-only Google Takeout planner for one decompressed
root or up to 64 independent split ZIP files. It streams archives without
extraction, reconciles title and supplemental sidecars, collapses
content-identical aliases, preserves album membership and emits source-neutral
description, UTC timestamp and location metadata in `normalized-plan-v2`.
Verification uses only synthetic fixtures and the pinned black-box oracle.
Its implementation and evidence SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933` is green in Gitea push CI
[run 5263](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5263).
Phase 8 now applies that normalized state through a separate source-aware
capability: bounded ZIP staging, asset upload, exact metadata assignment,
album creation/membership and effect-level `checkpoint-v2` resume. The
disposable HTTPS gate is green in
[run 5570](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5570),
and its paired benchmark is green in
[run 5578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5578).
See the [Phase 8 compatibility matrix](docs/compatibility/phase8-google-takeout-import.md).

Phase 4 adds read-only Apple Photos/iCloud directory and split-ZIP planning,
XMP and Live Photo pairing, preserve-all edited/original handling and explicit
album derivation in `normalized-plan-v3`. Phase 5 adds a loopback-only,
original-byte Immich archive with immutable manifests, bounded streaming,
checksum verification, atomic files and idempotent resume. See the
[Phase 4 matrix](docs/compatibility/phase4-apple-photos.md) and
[Phase 5 matrix](docs/compatibility/phase5-archive.md).

Apple Photos and Picasa imports now bind their read-only outputs to
`upload-plan-v2`, verify directory or split-ZIP inputs offline and reuse the
bounded effect-level import engine. Their disposable Immich, resume,
fresh-checkpoint convergence and pinned black-box differential gates are
complete. Immich-to-Immich migration likewise binds a read-only source and a
separate destination into `migration-plan-v1`, with one-file staging and
effect-level resume across two disposable servers. The external private-export
shadows remain pending until explicitly authorized exports are supplied. See
the [Apple](docs/compatibility/phase9-apple-photos-import.md),
[Picasa](docs/compatibility/phase10-picasa-import.md) and
[migration](docs/compatibility/phase11-immich-migration.md) matrices.
Apple/Picasa gate and benchmark
[run 5675](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5675)
is green on `f8c13eb4d9c7cfab968b1355004ae403a146d0f9`; the corrected
two-server migration
[run 5678](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5678)
is green on `88d9a59d1ca1d0cd92b3ef28349ff4b95a5f8095`.

The expanded synthetic image/XMP, video and live-photo matrix and its paired
raw upload benchmark are verified on implementation SHA
`3ed13d3293baf197fa5a21e624a828c201d7b763`: Gitea benchmark
[run 5234](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5234)
is green and published both reports. Evidence enforcement commit
`6975633920117948b709bd7e5170c1246bd73b89` is green in push CI
[run 5235](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5235).

## Release status

`0.1.0-rc.1` is the first supported release candidate. It includes the proven
read-only archive and immutable folder uploader against remote Immich v3.1.x
HTTPS endpoints, plus plan-bound Google Takeout, Apple Photos and Picasa
uploads with normalized metadata and albums. The source-import synthetic gates
are complete; their explicitly authorized private-export shadows remain
release evidence. Disposable Immich-to-Immich migration is implemented, while
production migration is not authorized. Delete, replace, trash and independent
maintenance remain unavailable. The
Phase 7 synthetic gate is green in
Gitea [run 5544](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5544)
on implementation SHA `db6ec2185b0e2bff8be8cb017f0fe8fafba95eb8`;
its [committed evidence](docs/evidence/phase7-disposable-production-2026-08-23.json)
is enforced by push CI.

ADR-0014/ADR-0025 are enforced by the fail-closed signed-tag pipeline,
deterministic native packaging, a hardened multiarch OCI candidate with SPDX
SBOM/SLSA provenance checks and a
[migration/rollback guide](docs/migration-from-immich-go.md). The committed
OpenPGP public identity matches the independently encrypted maintainer key, and
the protected release environment exposes only the three required secrets.

The macOS ARM64 native gate is complete on implementation SHA
`1168b17aa8349f76064e80471c0b72bf55009978`: manual Gitea
[run 5440](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5440)
ran 64 Python tooling tests, 76 Rust tests, Clippy with warnings denied and a
native release build, then identified the output as a Mach-O ARM64 executable.
The Windows x86-64 native gate is also green in Gitea
[run 5470](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5470)
on SHA `4cf3a4eddfb7dccc9072258ab44fb8ebf64a12b5`. GitHub
[run 33849192526](https://github.com/luigibarretta/immich-rs/actions/runs/33849192526)
then completed the non-publishing rehearsal on Linux x86-64/ARM64, macOS
x86-64/ARM64 and Windows x86-64 at exact revision
`f69a4c09bdeb09fbdae6d61260b0f8a20e2b6c3c`; Gitea run 6155 passed on the same
revision.

## Current capabilities

The optional operator Web Console now has a tested loopback and direct-TLS LAN
library surface. Loopback uses one-time bootstrap pairing; LAN mode uses OIDC
authorization code with PKCE and restart-ephemeral sessions. Source-only folder,
Google Takeout, Apple Photos and Picasa scan/review plus server-bound immutable
upload planning through opaque operator-configured profiles. Import profiles
select exactly one contained directory or a bounded contained split-ZIP set;
their adapter, archive, album and execution options are operator-owned and
bound into the profile generation. Work runs as bounded,
owned in-memory jobs with authenticated polling, cooperative cancellation and
joined shutdown; bounded authenticated SSE provides isolated replay with a
polling fallback, and terminal summaries are published only after a complete
plan. It applies exact Host/Origin/CSRF policy, contextual SSR escaping, private
response headers and bounded connections, headers, bodies and deadlines.
Operator configuration validates private state roots and exact server
profiles. Disposable profiles require a literal loopback address; remote
read-only HTTPS profiles require an explicit per-profile IP/CIDR allowlist, and
every bounded DNS answer must match it before addresses can be pinned without
weakening TLS hostname verification. Terminal scan history is now persisted in
a distinct bounded `console-history-v1` SQLite store under the private operator
state profile and exposed only to an
authenticated session; it contains only workflow/status enums, opaque plan
references and aggregate counters, never job/user identifiers or source data.
Server-bound planning uses a strictly bounded private secret loader and the
address-pinned read client. Completed plans are published atomically under
opaque references in private state, revalidated on every authenticated
inspection, and exported with bounded streaming. Authenticated folder and
source-import dry-run reopens and verifies that artifact entirely offline,
rejects source, state,
server-profile or credential-generation drift, creates no checkpoint, and
atomically records a versioned receipt with terminal history. Tests remove the
server credential and stop the disposable server before dry-run while proving
the request counter does not increase. Folder and source-import apply require a second exact
digest/count confirmation and a short-lived session-bound single-use grant.
Grant consumption and job admission are atomic and idempotent; executor-owned
apply performs another offline source/checkpoint verification before creating a
write-capable client, writes a private plan-bound checkpoint, and requires a
fresh dry-run receipt and confirmation after cancellation or restart. Replay,
expiry, logout, drift and bounded before/after-commit fault tests use only a
synthetic loopback server. Import tests exercise executor-owned apply and
fresh-plan duplicate convergence for a synthetic Takeout ZIP plus Apple and
Picasa directories, including metadata and album effects. The external private
Apple/Picasa shadow evidence remains pending and is not claimed. LAN tests use
only a disposable TLS identity and IdP. Exact claim/signature checks, replay,
key rotation, provider outage, fixation, expiry, SSE revocation, Origin/proxy
denial and private-address CIDR pinning are covered. Direct TLS is supported;
trusted reverse-proxy termination is not. Media-serving routes remain
unsupported. See the [LAN configuration contract](docs/web-console-lan.md). A
standalone web binary, native artifact and container are not yet supported. The
CLI remains the canonical complete interface. See the
[threat model](docs/web-console-threat-model.md) and
[capability/resource matrix](docs/web-console-resources.md).

- deterministic recursive folder discovery with bounded entry and path limits;
- one-file-at-a-time SHA-256 streaming through a configurable bounded buffer;
- NFC portable paths, case and Unicode collision diagnostics;
- explicit symlink, unreadable-file, special-entry and source-change outcomes;
- deterministic JSON/XMP sidecar and image/MOV live-photo candidates;
- stable operation, source and pair identities with explainable rule IDs;
- cooperative cancellation with no partial plan;
- synthetic golden, property and pinned black-box differential tests;
- paired raw process benchmarks on a reproducible 64 MiB synthetic corpus;
- immutable `upload-plan-v1` generation after authenticated version probing;
- bounded streaming upload with duplicate convergence and capped retries;
- durable SQLite checkpoints, clean cancellation and explicit dry-run;
- disposable Immich v3.1.0 and loopback mock integration gates;
- verified production HTTPS for folder upload and read-only archive, including
  bounded private-CA trust, exact plan/count/backup authorization and redaction;
- paired raw upload benchmarks against immich-go on one disposable server and
  one derived standalone synthetic corpus;
- decompressed or split-ZIP Google Takeout planning with bounded archive and
  JSON reads, deterministic metadata reconciliation and explicit ambiguity;
- source-neutral Takeout descriptions, UTC timestamps, locations and sorted
  album membership in `normalized-plan-v2`;
- immutable server-bound Takeout `upload-plan-v2` planning with byte-identical
  directory/ZIP output and an explicit maximum mutation budget;
- source-aware Takeout dry-run and apply with one-entry-at-a-time ZIP staging,
  exact source verification and `checkpoint-v2` effect journaling;
- distinct import-only metadata and album capabilities with bounded retries,
  lost-response reconciliation and exact production authorization;
- disposable Immich v3.1.0 postconditions plus paired plan-and-import evidence
  against the pinned black-box oracle;
- Apple Photos directory or split-ZIP planning with preserve-all variants,
  XMP, Live Photos, known-noise diagnostics and explicit album modes;
- immutable Apple Photos `upload-plan-v2`, offline source-aware dry-run,
  one-entry ZIP staging and plan-selected checkpointed import execution;
- bounded Picasa directory/split-ZIP planning and import with explicit
  `.picasa.ini` album/caption fields and optional filename-date fallback;
- immutable `migration-plan-v1` with separate read-only source and destination
  credentials, bounded one-original staging and effect-level resume;
- real disposable Apple, Picasa and two-server migration postconditions plus
  pinned black-box differential reports and comparable raw benchmarks;
- read-only Immich inventory and original-byte archive with immutable
  `archive-manifest-v1`, atomic writes and verified idempotent resume;
- strict schema-v1 TOML and `IMMICH_RS_*` configuration with deterministic
  `CLI > environment > file > default` precedence and redacted inspection;
- non-root, shell-free `linux/amd64` and `linux/arm64` OCI packaging plus a
  network-disabled Compose default and secret-backed opt-in HTTPS override.

| Source workflow | Plan/apply contract | Current gate |
|---|---|---|
| Folder (`from-folder`) | `normalized-plan-v1` / `upload-plan-v1` | Complete, including disposable and production-HTTPS boundaries |
| Google Photos (`from-google-photos`) | `normalized-plan-v2` / `upload-plan-v2` | Complete, including authorized aggregate Takeout shadow |
| Apple Photos (`from-icloud`) | `normalized-plan-v3` / `upload-plan-v2` | Synthetic/disposable/differential complete; private export shadow pending |
| Picasa (`from-picasa`) | `normalized-plan-v4` / `upload-plan-v2` | Synthetic/disposable/differential complete; private export shadow pending |
| Immich (`from-immich`) | `migration-plan-v1` / `checkpoint-v2` | Two-server disposable/differential complete; non-production shadow pending; production not authorized |

Render the effective non-secret configuration:

```bash
immich-rs --config /path/to/immich-rs.toml config show
```

See the complete [CLI/environment/TOML matrix](docs/configuration.md).

Build and run the hardened offline Compose planner:

```bash
IMMICH_RS_SOURCE_PATH=/absolute/path/to/authorized-source \
  docker compose build --pull
IMMICH_RS_SOURCE_PATH=/absolute/path/to/authorized-source \
  docker compose run --rm immich-rs > normalized-plan.json
docker compose down --volumes --remove-orphans
```

No supported image is published yet. The default service has no network,
runs as UID/GID 65532 and mounts the source read-only. Read the full
[container and Compose guide](docs/container.md) before overriding it.

Run a plan:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan folder --label synthetic-example /path/to/source
```

The normalized JSON plan is written to stdout. Errors use stderr and stable
exit classes. Running the same command over unchanged bytes and configuration
produces byte-identical output.

Plan a decompressed Google Takeout layout:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan google-takeout --label synthetic-takeout /path/to/export-root
```

Or plan independent parts of one split export without extracting them:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan google-takeout --label synthetic-takeout \
  /path/to/takeout-001.zip /path/to/takeout-002.zip
```

The directory root or ZIP entries must contain `Takeout/Google Photos`.
Directory and archive inputs cannot be mixed. This source-only command cannot
construct an HTTP client or upload capability.

Bind the same Takeout to a disposable server without applying it:

```bash
IMMICH_RS_API_KEY='<disposable-key>' \
  cargo run --locked --release -p immich-rs-cli -- \
  plan upload google-takeout --server http://127.0.0.1:2283 \
  --label synthetic-takeout /path/to/export-root > takeout-upload-plan.json
immich-rs inspect upload-plan --plan takeout-upload-plan.json
```

The server-aware command constructs only a read/probe capability. It emits
`upload-plan-v2` with asset, metadata, album-create, album-membership and
maximum mutation counts. Verify the unchanged source offline before applying:

```bash
immich-rs apply upload --dry-run \
  --plan takeout-upload-plan.json --source /path/to/export-root \
  --checkpoint takeout-checkpoint.sqlite
```

Dry-run reads no credential, creates no checkpoint and cannot construct a
network client. Actual apply rescans the exact source, streams one ZIP entry at
a time when needed and journals each upload, metadata and album effect in
`checkpoint-v2`. Split archives use repeated `--input` instead of `--source`.
Remote HTTPS execution requires the same exact plan digest, maximum mutation
count and backup confirmation shown for folder upload below.

Plan an Apple Photos export without uploading it:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan apple-photos --album-mode folder /path/to/export-or-icloud-part.zip
```

Bind the same export to an authorized disposable server, inspect it and verify
it offline before apply:

```bash
IMMICH_RS_API_KEY='<disposable-key>' \
  immich-rs plan upload apple-photos \
  --server http://127.0.0.1:2283 --album-mode folder \
  /path/to/export-or-icloud-part.zip > apple-upload-plan.json
immich-rs inspect upload-plan --plan apple-upload-plan.json
immich-rs apply upload --dry-run --plan apple-upload-plan.json \
  --source /path/to/export-or-icloud-part.zip \
  --album-mode folder --checkpoint apple-checkpoint.sqlite
```

For split ZIP downloads, repeat `--input` during apply. The immutable plan
selects the Apple adapter; a caller cannot reinterpret it as folder or Takeout
input. Filesystem or ZIP timestamps are transport fields only and are never
promoted to normalized capture metadata.

Plan and dry-run a Picasa export with explicitly bounded metadata behavior:

```bash
IMMICH_RS_API_KEY='<disposable-key>' \
  immich-rs plan upload picasa --server http://127.0.0.1:2283 \
  --picasa-albums --filename-date /path/to/picasa-export \
  > picasa-upload-plan.json
immich-rs apply upload --dry-run --plan picasa-upload-plan.json \
  --source /path/to/picasa-export --picasa-albums --filename-date \
  --checkpoint picasa-checkpoint.sqlite
```

Only the bounded Picasa album name, per-file caption and optional filename
date enter the plan. Unknown INI data cannot authorize tags, people,
favorites, stacks or destructive mutations.

Archive originals from an isolated loopback Immich instance:

```bash
IMMICH_RS_API_KEY='<disposable-key>' \
  cargo run --locked --release -p immich-rs-cli -- \
  plan archive immich --server http://127.0.0.1:2283 > archive-manifest.json
IMMICH_RS_API_KEY='<disposable-key>' \
  cargo run --locked --release -p immich-rs-cli -- \
  apply archive --server http://127.0.0.1:2283 \
  --manifest archive-manifest.json --destination /path/to/archive
```

The archive command reads originals only. It cannot upload, replace, delete or
mutate server metadata. A remote HTTPS origin additionally requires the
CLI-only `--authorize-production-read` acknowledgement.

Create and verify a disposable Immich-to-Immich migration plan:

```bash
IMMICH_RS_SOURCE_API_KEY='<disposable-source-key>' \
IMMICH_RS_DESTINATION_API_KEY='<disposable-destination-key>' \
  immich-rs plan migration immich \
  --source-server http://127.0.0.1:2284 \
  --destination-server http://127.0.0.1:2285 \
  --max-assets 1000 --max-total-bytes 107374182400 \
  > migration-plan.json
immich-rs apply migration immich --dry-run \
  --plan migration-plan.json --max-assets 1000 \
  --max-total-bytes 107374182400
```

Live migration additionally requires the two distinct keys, both exact
loopback origins and a checkpoint. Production migration is deliberately
rejected by the current ADR.

Plan and validate a disposable upload before applying it:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan upload folder --server http://127.0.0.1:2283 \
  --label synthetic-example /path/to/source > upload-plan.json
cargo run --locked --release -p immich-rs-cli -- \
  apply upload --dry-run --plan upload-plan.json \
  --source /path/to/source --checkpoint checkpoint.sqlite
cargo run --locked --release -p immich-rs-cli -- \
  apply upload --server http://127.0.0.1:2283 --plan upload-plan.json \
  --source /path/to/source --checkpoint checkpoint.sqlite
```

The two server-aware commands read `IMMICH_RS_API_KEY` or the regular secret
file selected by `IMMICH_RS_API_KEY_FILE`; dry-run reads neither and cannot
construct a network client. Use only credentials generated for a disposable
instance.

For a remote HTTPS folder upload, first create and inspect the immutable plan:

```bash
IMMICH_RS_API_KEY_FILE=/run/secrets/immich-api-key \
  immich-rs plan upload folder \
  --server https://immich.example.invalid \
  --authorize-production-read \
  /path/to/source > upload-plan.json
immich-rs inspect upload-plan --plan upload-plan.json
immich-rs apply upload --dry-run --plan upload-plan.json \
  --source /path/to/source --checkpoint checkpoint.sqlite
```

After independently verifying the printed digest, operation count and a usable
backup or restore point, apply that exact plan:

```bash
IMMICH_RS_API_KEY_FILE=/run/secrets/immich-api-key \
  immich-rs apply upload \
  --server https://immich.example.invalid \
  --plan upload-plan.json --source /path/to/source \
  --checkpoint checkpoint.sqlite \
  --authorize-production-read --authorize-production-write \
  --confirm-plan-sha256 <64-hex-digest> \
  --expected-operations <count> \
  --backup-reference <verified-restore-point-reference>
```

Add `--ca-certificate /path/to/private-ca.pem` only when the HTTPS deployment
uses a private CA; hostname verification remains mandatory. All production
acknowledgements are invocation-only and cannot be enabled through TOML,
environment variables or Compose. See the
[Phase 7 matrix](docs/compatibility/phase7-production-https.md).

## Safety boundary

- The workspace forbids `unsafe`, `unwrap` and `expect`.
- Maintained Rust, Python and shell files have a 400-LOC hard limit with no
  baseline exceptions.
- Media are never buffered as whole files; configured limits fail closed.
- Source-only folder, Takeout, Apple and Picasa planners plus upload dry-run cannot
  construct an HTTP client or mutation capability. Server-bound upload
  planners can only construct a read/probe capability; Takeout, Apple and Picasa
  metadata/album methods exist only on the separately authorized import client.
- The migration source capability has no mutation methods, its key must differ
  from the destination key and the current transport accepts only two distinct
  disposable loopback origins.
- Disposable transport accepts only literal loopback origins. Production
  transport accepts only verified remote HTTPS and requires explicit CLI-only
  read authorization; upload also requires the exact write confirmation set.
- API keys exist only in `IMMICH_RS_API_KEY` or the bounded regular file named
  by `IMMICH_RS_API_KEY_FILE` and are redacted from outputs.
- Committed fixture media, API responses, credentials and identities are
  synthetic. Authorized private shadow evidence contains aggregate counters
  only and no paths, names, metadata or content digests.
- Production host patterns and credential-shaped values are rejected by the
  repository safety check.

The Go executable is used exclusively as an external black-box oracle. No Go
source is copied, translated or linked. CI fetches the official immich-go
v0.32.0 release only on a cache miss, verifies the pinned archive and binary
digests, and never publishes the executable as an artifact.

The observed oracle sends five job-resume PUT requests despite `--dry-run
--pause-immich-jobs=false`. The exact calls are classified as an oracle defect
and contained by the loopback mock; any drift or asset mutation fails closed.
See the [Phase 1 compatibility matrix](docs/compatibility/phase1-folder.md).
The upload boundary and current evidence are recorded in the
[Phase 2 gate matrix](docs/compatibility/phase2-folder-upload.md).
The bounded Takeout surface is recorded in the
[Phase 3 compatibility matrix](docs/compatibility/phase3-google-takeout.md).
The Apple and archive boundaries are recorded in the
[Phase 4](docs/compatibility/phase4-apple-photos.md) and
[Phase 5](docs/compatibility/phase5-archive.md) matrices. Remote transport and
operator binding are recorded in the
[Phase 7 matrix](docs/compatibility/phase7-production-https.md). Source-aware
Takeout execution is recorded in the
[Phase 8 matrix](docs/compatibility/phase8-google-takeout-import.md).
Apple execution status is recorded separately in the
[Apple import matrix](docs/compatibility/phase9-apple-photos-import.md).
Picasa execution and Immich-to-Immich migration are recorded in the
[Phase 10](docs/compatibility/phase10-picasa-import.md) and
[Phase 11](docs/compatibility/phase11-immich-migration.md) matrices.

## Baselines

| Contract | Pinned baseline |
|---|---|
| Behavioral oracle | immich-go v0.32.0, commit `f7d19fce34acd4884ea2c02fc3025706a060afdf` |
| First Immich target | Immich v3.1 synthetic mock fixtures |
| Rust toolchain | 1.88.0, edition 2024 |
| License | AGPL-3.0-only |
| Normalized plan | `normalized-plan-v1` for folder/upload; `normalized-plan-v2` for Takeout; `normalized-plan-v3` for Apple Photos; `normalized-plan-v4` for Picasa |
| Upload plan | `upload-plan-v1` for folder apply; `upload-plan-v2` for Google Takeout, Apple Photos and Picasa apply |
| Upload checkpoint | `checkpoint-v1` for folder upload; effect-level `checkpoint-v2` for source-aware imports |
| Migration | `migration-plan-v1`; effect-level `checkpoint-v2` |
| Read-only archive | `archive-manifest-v1`; `archive-apply-report-v1` |
| Disposable Immich | exact v3.1.x release, currently v3.1.0 |

## Workspace

- `immich-cli`: explicit plan, dry-run and loopback apply command surface;
- `immich-application`: thin typed workflow composition shared by both
  frontends;
- `immich-web`: authenticated SSR/HTTP, configured profiles, bounded jobs,
  private history and short-lived grant policy;
- `immich-core`: source-neutral plans, diagnostics, events and cancellation;
- `immich-client`: version-aware HTTP boundary with distinct opaque read,
  archive, folder-upload and source-import capabilities;
- `immich-executor`: immutable upload/archive planning, verification, retry,
  checkpoint and atomic-file orchestration;
- `immich-sources`: bounded folder, Google Takeout, Apple Photos and Picasa discovery
  and reconciliation.

ADR-0033 accepts the implemented `immich-application` and `immich-web` crates.
The CLI and executor contracts remain authoritative and compatible.

The crate boundaries are dependency rules, not microservices. See
[ADR-0004](docs/adr/ADR-0004-modular-workspace-architecture.md).

## Verification

Run the complete fast gate:

```bash
scripts/check-adrs.sh
python3 scripts/check-architecture.py
python3 scripts/check-benchmark-evidence.py
python3 scripts/check-disposable-evidence.py
python3 scripts/check-phase5-evidence.py
python3 scripts/check-phase6-evidence.py
python3 scripts/check-production-evidence.py
python3 scripts/check-phase8-evidence.py
python3 scripts/check-phase8-benchmark.py
python3 scripts/check-source-import-evidence.py
python3 scripts/check-source-import-benchmark.py
python3 scripts/check-migration-evidence.py
python3 scripts/check-real-migration-benchmark.py
python3 scripts/check-release.py
python3 scripts/check-container.py
python3 scripts/check-fixtures.py
python3 scripts/check-loc.py
python3 -m unittest discover -s tests/tooling -p 'test_*.py'
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo doc --locked --workspace --no-deps
cargo deny check
cargo audit --deny warnings
```

On Windows x64, run the Python entries with `py -3.12` and then validate the
native release executable explicitly:

```powershell
cargo build --locked --release -p immich-rs-cli
py -3.12 scripts/check-pe.py target/release/immich-rs.exe
```

The pinned differential additionally runs:

```bash
python3 scripts/run-oracle.py tests/oracle/cases/folder-matrix-v1.json \
  --oracle /path/to/verified/immich-go \
  --output /path/to/observation.json
python3 scripts/compare-oracle.py \
  tests/oracle/compatibility/folder-matrix-v1.json \
  /path/to/observation.json
python3 scripts/run-oracle.py \
  tests/oracle/cases/google-takeout-basic-v1.json \
  --oracle /path/to/verified/immich-go \
  --output /path/to/takeout-observation.json
python3 scripts/compare-takeout-oracle.py \
  tests/oracle/compatibility/google-takeout-basic-v1.json \
  /path/to/takeout-observation.json
python3 scripts/run-oracle.py \
  tests/oracle/cases/google-takeout-complete-v2.json \
  --oracle /path/to/verified/immich-go \
  --output /path/to/takeout-complete-observation.json
python3 scripts/compare-takeout-complete.py \
  tests/oracle/compatibility/google-takeout-complete-v2.json \
  /path/to/takeout-complete-observation.json
```

The full Phase 2 disposable gate is manual and removes its exact containers,
volumes, network, temporary workspace and test-only images on every exit:

```bash
scripts/run-disposable-immich.sh \
  --binary target/release/immich-rs \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/phase2-disposable.json
```

The Phase 5 disposable archive gate uses the same cleanup contract, verifies
source/archive byte equality and can run the paired comparison in one isolated
server:

```bash
scripts/run-disposable-archive.sh \
  --binary target/release/immich-rs \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/phase5-disposable.json \
  --oracle .cache/oracle/immich-go-0.32.0-linux-x86_64/immich-go \
  --benchmark-output .artifacts/phase5-benchmark.json
```

The isolated Phase 7 HTTPS gate must run only against a disposable server. It
creates a private CA and synthetic owner/key, injects transport faults and
removes its exact containers, volumes, network, workspace and credentials:

```bash
scripts/run-disposable-production.sh \
  --binary target/release/immich-rs \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/phase7-production.json
python3 scripts/check-production-evidence.py \
  --input .artifacts/phase7-production.json
```

The Phase 8 gate performs the same disposable cleanup after directory and
split-ZIP import, normalized metadata and album postcondition checks:

```bash
scripts/run-disposable-takeout.sh \
  --binary target/release/immich-rs \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/phase8-takeout.json
python3 scripts/check-phase8-evidence.py \
  --input .artifacts/phase8-takeout.json
```

The Apple/Picasa source gate and the two-server migration gate are also manual
and disposable:

```bash
scripts/run-disposable-source-import.sh --adapter apple-photos \
  --binary target/release/immich-rs --oracle /path/to/verified/immich-go \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/apple-source-import.json
scripts/run-disposable-source-import.sh --adapter picasa \
  --binary target/release/immich-rs --oracle /path/to/verified/immich-go \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/picasa-source-import.json
scripts/run-disposable-migration.sh --binary target/release/immich-rs \
  --oracle /path/to/verified/immich-go \
  --commit-sha "$(git rev-parse HEAD)" \
  --output .artifacts/immich-migration.json
```

Full paired benchmarks are manual and separate from push CI. Read the
[methodology](benchmarks/README.md) and the committed
[Phase 1](benchmarks/evidence/phase1-2026-08-14.json) and
[Phase 2](benchmarks/evidence/phase2-2026-08-15.json) upload evidence plus the
[Phase 3](benchmarks/evidence/phase3-2026-08-15.json) Takeout,
[Phase 4](benchmarks/evidence/phase4-2026-08-21.json) Apple and
[Phase 5](benchmarks/evidence/phase5-2026-08-22.json) archive evidence, plus
the complete [Phase 8](benchmarks/evidence/phase8-2026-08-24.json) Takeout
import comparison and the complete [Apple](benchmarks/evidence/phase9-source-import-benchmark-2026-08-24.json),
[Picasa](benchmarks/evidence/phase10-source-import-benchmark-2026-08-24.json)
and [migration](benchmarks/evidence/phase11-real-2026-08-24.json) reports.
Those measurements are not generalized performance claims.
The
Phase 2 harness completed in Gitea run 5234 and uploaded the raw benchmark plus
disposable cleanup evidence for its exact implementation SHA. The Phase 3
harness completed in Gitea run 5264 for implementation SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933`. The Phase 8 harness completed in
Gitea run 5578 for implementation SHA
`4edae0365ac5137a2ee99d216755d8e3693cf9c8`.

Read [ROADMAP.md](ROADMAP.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the full
[ADR index](docs/adr/README.md) before implementing a new vertical.

## Project relationship

`immich-rs` is an independent community project. It is not affiliated with or
endorsed by the Immich or immich-go projects. Their names and APIs remain the
property of their respective owners.
