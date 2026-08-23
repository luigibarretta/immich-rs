# immich-rs

`immich-rs` is an independent Rust implementation of a high-integrity import,
archive and migration client for [Immich](https://immich.app/).

## Performance at a glance

Two reproducible comparisons currently satisfy ADR-0012's threshold for a
scoped performance statement against the pinned immich-go v0.32.0 oracle:

| Lower is better | immich-rs | immich-go | Difference |
|---|---:|---:|---:|
| Folder scan/plan median | 49.371 ms | 57.624 ms | immich-rs 14.3% lower |
| Folder scan/plan p95 | 51.393 ms | 58.321 ms | immich-rs 11.9% lower |
| Read-only archive median | 40.964 ms | 93.808 ms | immich-rs 56.3% lower |
| Read-only archive p95 | 55.234 ms | 101.784 ms | immich-rs 45.7% lower |
| Archive median peak RSS | 6.47 MiB | 14.92 MiB | immich-rs 56.6% lower |

The folder comparison uses the same page-cached deterministic 64 MiB,
eight-asset synthetic corpus. The archive comparison uses the same owner and
four standalone originals totalling 587,015 bytes on one disposable Immich
v3.1.0 server with a warm cache. Both use concurrency one, alternating order,
two warmups and six retained pairs. In both comparisons every retained
immich-rs wall-time sample is below every immich-go sample.

These are small synthetic CPU/network-loopback results. They do not measure a
large library, cold storage, production latency, Takeout planning or an
end-to-end migration. See the [Phase 1 raw report](benchmarks/evidence/phase1-2026-08-21.json),
the [Phase 5 raw report](benchmarks/evidence/phase5-2026-08-22.json) and the
full [benchmark methodology](benchmarks/README.md). No broader speed claim is
supported by these measurements.

Phase 0 and Phase 1 are complete for implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`: push CI
[run 5165](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5165)
and manual benchmark
[run 5170](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5170)
are green. The first Phase 2 vertical adds an explicit, idempotent folder-upload
workflow restricted to disposable loopback Immich instances. Delete, replace
and independent metadata mutation remain unavailable.

Phase 3 completes the read-only Google Takeout planner for one decompressed
root or up to 64 independent split ZIP files. It streams archives without
extraction, reconciles title and supplemental sidecars, collapses
content-identical aliases, preserves album membership and emits source-neutral
description, UTC timestamp and location metadata in `normalized-plan-v2`.
Verification uses only synthetic fixtures and the pinned black-box oracle.
Takeout apply remains unavailable. Implementation and evidence SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933` is green in Gitea push CI
[run 5263](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5263).
A later read-only planning vertical emits a server-bound `upload-plan-v2` with
normalized metadata, album membership and the exact maximum mutation budget.
Its implementation SHA `b43a21b614f1a49145ea788e9d2011acf49fa741`
is green in Gitea push CI
[run 5554](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5554).
The generic apply command rejects this schema before reading a credential or
reaching a server; this is not yet Takeout import support.

Phase 4 adds read-only Apple Photos/iCloud directory and split-ZIP planning,
XMP and Live Photo pairing, preserve-all edited/original handling and explicit
album derivation in `normalized-plan-v3`. Phase 5 adds a loopback-only,
original-byte Immich archive with immutable manifests, bounded streaming,
checksum verification, atomic files and idempotent resume. See the
[Phase 4 matrix](docs/compatibility/phase4-apple-photos.md) and
[Phase 5 matrix](docs/compatibility/phase5-archive.md).

The expanded synthetic image/XMP, video and live-photo matrix and its paired
raw upload benchmark are verified on implementation SHA
`3ed13d3293baf197fa5a21e624a828c201d7b763`: Gitea benchmark
[run 5234](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5234)
is green and published both reports. Evidence enforcement commit
`6975633920117948b709bd7e5170c1246bd73b89` is green in push CI
[run 5235](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5235).

## Release status

No supported release is published yet. Phase 7 now permits a source-built
client to use the proven read-only archive and immutable folder uploader against
a remote Immich v3.1.x HTTPS endpoint, but it does not authorize Takeout/Apple
apply, delete, replace or metadata mutation. The synthetic gate is green in
Gitea [run 5544](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5544)
on implementation SHA `db6ec2185b0e2bff8be8cb017f0fe8fafba95eb8`;
its [committed evidence](docs/evidence/phase7-disposable-production-2026-08-23.json)
is enforced by push CI.

ADR-0014/ADR-0025 still require five native target builds and an explicitly
provisioned OpenPGP release identity before an RC can exist. The repository
contains a fail-closed signed-tag pipeline, deterministic native packaging, a
hardened multiarch OCI candidate with SPDX SBOM/SLSA provenance checks and a
[migration/rollback guide](docs/migration-from-immich-go.md).

The macOS ARM64 native gate is complete on implementation SHA
`1168b17aa8349f76064e80471c0b72bf55009978`: manual Gitea
[run 5440](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5440)
ran 64 Python tooling tests, 76 Rust tests, Clippy with warnings denied and a
native release build, then identified the output as a Mach-O ARM64 executable.
The Windows x86-64 native gate is also green in Gitea
[run 5470](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5470)
on SHA `4cf3a4eddfb7dccc9072258ab44fb8ebf64a12b5`. The remaining RC
prerequisites are native Linux ARM64 and macOS x86-64 runners, the armored
maintainer public key, three protected signing secrets and one non-publishing
five-target rehearsal. No RC is published yet.

## Current capabilities

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
- Apple Photos directory or split-ZIP planning with preserve-all variants,
  XMP, Live Photos, known-noise diagnostics and explicit album modes;
- read-only Immich inventory and original-byte archive with immutable
  `archive-manifest-v1`, atomic writes and verified idempotent resume;
- strict schema-v1 TOML and `IMMICH_RS_*` configuration with deterministic
  `CLI > environment > file > default` precedence and redacted inspection;
- non-root, shell-free `linux/amd64` and `linux/arm64` OCI packaging plus a
  network-disabled Compose profile for offline planning.

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
maximum mutation counts. Both dry-run and mutating apply currently reject this
schema before reading credentials or reaching a server.

Plan an Apple Photos export without uploading it:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan apple-photos --album-mode folder /path/to/export-or-icloud-part.zip
```

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
- Source-only folder, Takeout and Apple planners plus upload dry-run cannot
  construct an HTTP client or upload capability. Server-bound upload planners
  can only construct a read/probe capability.
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
[Phase 7 matrix](docs/compatibility/phase7-production-https.md).

## Baselines

| Contract | Pinned baseline |
|---|---|
| Behavioral oracle | immich-go v0.32.0, commit `f7d19fce34acd4884ea2c02fc3025706a060afdf` |
| First Immich target | Immich v3.1 synthetic mock fixtures |
| Rust toolchain | 1.88.0, edition 2024 |
| License | AGPL-3.0-only |
| Normalized plan | `normalized-plan-v1` for folder/upload; `normalized-plan-v2` for Takeout; `normalized-plan-v3` for Apple Photos |
| Upload plan | `upload-plan-v1` for folder apply; read-only `upload-plan-v2` planning for Takeout |
| Upload checkpoint | `checkpoint-v1` |
| Read-only archive | `archive-manifest-v1`; `archive-apply-report-v1` |
| Disposable Immich | exact v3.1.x release, currently v3.1.0 |

## Workspace

- `immich-cli`: explicit plan, dry-run and loopback apply command surface;
- `immich-core`: source-neutral plans, diagnostics, events and cancellation;
- `immich-client`: version-aware HTTP boundary with distinct opaque read,
  archive and upload capabilities;
- `immich-executor`: immutable upload/archive planning, verification, retry,
  checkpoint and atomic-file orchestration;
- `immich-sources`: bounded folder, Google Takeout and Apple Photos discovery
  and reconciliation.

The crate boundaries are dependency rules, not microservices. See
[ADR-0004](docs/adr/ADR-0004-modular-workspace-architecture.md).

## Verification

Run the complete fast gate:

```bash
python3 scripts/check-adrs.py
python3 scripts/check-architecture.py
python3 scripts/check-benchmark-evidence.py
python3 scripts/check-disposable-evidence.py
python3 scripts/check-phase5-evidence.py
python3 scripts/check-phase6-evidence.py
python3 scripts/check-production-evidence.py
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

Full paired benchmarks are manual and separate from push CI. Read the
[methodology](benchmarks/README.md) and the committed
[Phase 1](benchmarks/evidence/phase1-2026-08-14.json) and
[Phase 2](benchmarks/evidence/phase2-2026-08-15.json) upload evidence plus the
[Phase 3](benchmarks/evidence/phase3-2026-08-15.json) Takeout,
[Phase 4](benchmarks/evidence/phase4-2026-08-21.json) Apple and
[Phase 5](benchmarks/evidence/phase5-2026-08-22.json) archive evidence. Those
measurements are not generalized performance claims. The
Phase 2 harness completed in Gitea run 5234 and uploaded the raw benchmark plus
disposable cleanup evidence for its exact implementation SHA. The Phase 3
harness completed in Gitea run 5264 for implementation SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933`.

Read [ROADMAP.md](ROADMAP.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the full
[ADR index](docs/adr/README.md) before implementing a new vertical.

## Project relationship

`immich-rs` is an independent community project. It is not affiliated with or
endorsed by the Immich or immich-go projects. Their names and APIs remain the
property of their respective owners.
