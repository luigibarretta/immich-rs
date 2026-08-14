# immich-rs

`immich-rs` is an independent Rust implementation of a high-integrity import,
archive and migration client for [Immich](https://immich.app/).

The current executable is deliberately read-only. It recursively scans a real
folder and emits a versioned normalized plan; it has no upload, delete, replace
or metadata-mutation command and no dependency path to the Immich client crate.
Phase 2 mutation work remains out of scope until the final read-only gate is
green for the exact implementation SHA in Gitea.

## Current capabilities

- deterministic recursive folder discovery with bounded entry and path limits;
- one-file-at-a-time SHA-256 streaming through a configurable bounded buffer;
- NFC portable paths, case and Unicode collision diagnostics;
- explicit symlink, unreadable-file, special-entry and source-change outcomes;
- deterministic JSON/XMP sidecar and image/MOV live-photo candidates;
- stable operation, source and pair identities with explainable rule IDs;
- cooperative cancellation with no partial plan;
- synthetic golden, property and pinned black-box differential tests;
- paired raw process benchmarks on a reproducible 64 MiB synthetic corpus.

Run a plan:

```bash
cargo run --locked --release -p immich-rs-cli -- \
  plan folder --label synthetic-example /path/to/source
```

The normalized JSON plan is written to stdout. Errors use stderr and stable
exit classes. Running the same command over unchanged bytes and configuration
produces byte-identical output.

## Safety boundary

- The workspace forbids `unsafe`, `unwrap` and `expect`.
- Maintained Rust, Python and shell files have a 400-LOC hard limit with no
  baseline exceptions.
- Media are never buffered as whole files; configured limits fail closed.
- Phase 1 CLI dependencies cannot reach `immich-client`.
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

## Baselines

| Contract | Pinned baseline |
|---|---|
| Behavioral oracle | immich-go v0.32.0, commit `f7d19fce34acd4884ea2c02fc3025706a060afdf` |
| First Immich target | Immich v3.1 synthetic mock fixtures |
| Rust toolchain | 1.88.0, edition 2024 |
| License | AGPL-3.0-only |
| Normalized plan | `normalized-plan-v1` |

## Workspace

- `immich-cli`: read-only command surface and exit behavior;
- `immich-core`: source-neutral plans, diagnostics, events and cancellation;
- `immich-client`: isolated future version-aware HTTP boundary, not reachable
  from the Phase 1 CLI;
- `immich-sources`: bounded folder discovery and reconciliation.

The crate boundaries are dependency rules, not microservices. See
[ADR-0004](docs/adr/ADR-0004-modular-workspace-architecture.md).

## Verification

Run the complete fast gate:

```bash
scripts/check-adrs.sh
python3 scripts/check-architecture.py
python3 scripts/check-benchmark-evidence.py
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

Full paired benchmarks are manual and separate from push CI. Read the
[methodology](benchmarks/README.md) and the committed
[raw evidence](benchmarks/evidence/phase1-2026-08-14.json). Those measurements
are not a generalized performance claim.

Read [ROADMAP.md](ROADMAP.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the full
[ADR index](docs/adr/README.md) before implementing a new vertical.

## Project relationship

`immich-rs` is an independent community project. It is not affiliated with or
endorsed by the Immich or immich-go projects. Their names and APIs remain the
property of their respective owners.
