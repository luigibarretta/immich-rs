# immich-rs

`immich-rs` is an independent Rust implementation of a high-integrity import,
archive and migration client for [Immich](https://immich.app/).

Phase 0 and Phase 1 are complete for implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`: push CI
[run 5165](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5165)
and manual benchmark
[run 5170](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5170)
are green. The first Phase 2 vertical adds an explicit, idempotent folder-upload
workflow restricted to disposable loopback Immich instances. Delete, replace
and independent metadata mutation remain unavailable.

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
- disposable Immich v3.1.0 and loopback mock integration gates.

Run a plan:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan folder --label synthetic-example /path/to/source
```

The normalized JSON plan is written to stdout. Errors use stderr and stable
exit classes. Running the same command over unchanged bytes and configuration
produces byte-identical output.

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

The two server-aware commands read `IMMICH_RS_API_KEY`; dry-run neither reads
the key nor constructs a network client. Use only credentials generated for a
disposable instance.

## Safety boundary

- The workspace forbids `unsafe`, `unwrap` and `expect`.
- Maintained Rust, Python and shell files have a 400-LOC hard limit with no
  baseline exceptions.
- Media are never buffered as whole files; configured limits fail closed.
- Phase 1 and upload dry-run dependency paths cannot construct an HTTP client.
- Phase 2 accepts only `127.0.0.1`, `[::1]` or `localhost` server origins.
- API keys exist only in `IMMICH_RS_API_KEY` and are redacted from outputs.
- Test media, API responses, credentials and identities are synthetic.
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

## Baselines

| Contract | Pinned baseline |
|---|---|
| Behavioral oracle | immich-go v0.32.0, commit `f7d19fce34acd4884ea2c02fc3025706a060afdf` |
| First Immich target | Immich v3.1 synthetic mock fixtures |
| Rust toolchain | 1.88.0, edition 2024 |
| License | AGPL-3.0-only |
| Normalized plan | `normalized-plan-v1` |
| Upload plan | `upload-plan-v1` |
| Upload checkpoint | `checkpoint-v1` |
| Disposable Immich | exact v3.1.x release, currently v3.1.0 |

## Workspace

- `immich-cli`: explicit plan, dry-run and loopback apply command surface;
- `immich-core`: source-neutral plans, diagnostics, events and cancellation;
- `immich-client`: version-aware HTTP boundary with opaque read and upload
  capabilities;
- `immich-executor`: immutable upload planning, verification, retry and journal
  orchestration;
- `immich-sources`: bounded folder discovery and reconciliation.

The crate boundaries are dependency rules, not microservices. See
[ADR-0004](docs/adr/ADR-0004-modular-workspace-architecture.md).

## Verification

Run the complete fast gate:

```bash
scripts/check-adrs.sh
python3 scripts/check-architecture.py
python3 scripts/check-benchmark-evidence.py
python3 scripts/check-disposable-evidence.py
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

The pinned differential additionally runs:

```bash
python3 scripts/run-oracle.py tests/oracle/cases/folder-matrix-v1.json \
  --oracle /path/to/verified/immich-go \
  --output /path/to/observation.json
python3 scripts/compare-oracle.py \
  tests/oracle/compatibility/folder-matrix-v1.json \
  /path/to/observation.json
```

The full Phase 2 disposable gate is manual and removes its exact containers,
volumes, network, temporary workspace and test-only images on every exit:

```bash
scripts/run-disposable-immich.sh \
  --binary target/release/immich-rs \
  --commit-sha "$(git rev-parse HEAD)" \
  --output /path/to/phase2-disposable.json
```

Full paired benchmarks are manual and separate from push CI. Read the
[methodology](benchmarks/README.md) and the committed
[raw evidence](benchmarks/evidence/phase1-2026-08-14.json). Those measurements
are not a generalized performance claim. The same harness completed in Gitea
run 5170 and uploaded the raw JSON evidence for the exact implementation SHA.

Read [ROADMAP.md](ROADMAP.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the full
[ADR index](docs/adr/README.md) before implementing a new vertical.

## Project relationship

`immich-rs` is an independent community project. It is not affiliated with or
endorsed by the Immich or immich-go projects. Their names and APIs remain the
property of their respective owners.
