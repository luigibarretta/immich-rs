# Benchmark methodology

## Phase 1 scan and plan

`scripts/benchmark-phase1.py` measures immich-rs and the pinned immich-go
oracle on the same materialized 64 MiB synthetic corpus inside each pair.
Fixture generation and mock startup are outside the measured interval. The
order alternates per sample to bound page-cache ordering bias. Both tools use
one worker; immich-rs uses a 65,536-byte media buffer.

The Linux process sampler records raw wall time, user and system CPU, peak RSS,
peak open file descriptors, procfs character/storage I/O, logical media bytes,
operation counts and observed identical-request retries. Output is captured in
bounded temporary files rather than pipes. The report includes binary,
fixture, expected-plan and source-revision digests plus the non-identifying
environment manifest.

Run the full manual suite after building the release binary:

```bash
SOURCE_REVISION=<exact-commit-sha> python3 scripts/benchmark-phase1.py \
  --samples 6 \
  --warmups 2 \
  --oracle /path/to/verified/immich-go \
  --output benchmarks/results/phase1.json
```

The Gitea `phase1-benchmark` workflow performs the same run manually and
publishes only JSON evidence, never the oracle executable. Raw results do not
constitute a performance claim. ADR-0012 budgets apply before any improvement
is advertised.

Current verification record: the release binary for implementation SHA
`03e6e13855ad4b401143653c30fa352fc8761546` completed ten paired samples
after two warmups. The committed
[raw report](evidence/phase1-2026-09-10.json) records the 64 MiB, eight-asset
fixture, both binary digests and the pinned immich-go v0.32.0 identity. Gitea
[run 6578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/6578)
validated the report on evidence SHA
`60a860995584334ecaa38d281b48499b6caaddc4`.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.087376 | 0.097891 | 0.068769 | 0.085232 |
| Peak RSS (bytes) | 5,220,352 | 5,701,632 | 16,130,048 | 18,796,544 |

The latest wall-time result favors immich-go, while the raw immich-rs RSS
measurements are lower. This report makes no performance-improvement claim.
The faster immich-rs result in the earlier 2026-08-21 report remains historical
evidence and is not presented as the current release-rehearsal result.

## Phase 2 folder upload

`scripts/benchmark-phase2.py` measures the public immich-rs plan-plus-apply
workflow and the pinned immich-go `upload from-folder` command against fresh
synthetic owners on one disposable Immich v3.1.0 server. Every pair uses the
same versioned corpus, loopback forwarder, server and environment. Both tools
use concurrency one and execution order alternates. Account creation, server
startup, fixture generation and postcondition probes are outside the measured
interval.

The correctness corpus contains one JPEG with XMP, one standalone MP4 and one
linked JPEG/MOV live photo. The performance view derives four standalone assets
from the same bytes and replaces the two fixed-size synthetic Apple identifiers
in a bounded stream. This keeps observable server outcomes equal without
claiming live-photo semantic parity for immich-go.

The release rehearsal completed six recorded pairs after two warmups on
implementation SHA `03e6e13855ad4b401143653c30fa352fc8761546`.
Every tool/sample produced four visible assets, zero live-photo links and zero
retries. These are the committed raw aggregates:

| Metric | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.150688 | 0.165093 | 0.153730 | 0.256081 |
| User CPU (s) | 0.008911 | 0.023337 | 0.011209 | 0.011743 |
| System CPU (s) | 0.011966 | 0.029378 | 0.011219 | 0.015333 |
| Peak RSS (bytes) | 10,295,296 | 10,383,360 | 13,891,584 | 15,036,416 |
| Peak file descriptors | 13 | 14 | 16 | 16 |
| Characters read | 2,374,825 | 2,374,829 | 1,306,718 | 1,307,022 |
| Characters written | 688,957 | 723,220 | 603,944 | 603,992 |
| Storage bytes read | 0 | 73,728 | 0 | 0 |
| Storage bytes written | 0 | 0 | 0 | 0 |

The wall-time ranges overlap, so the report makes no performance-improvement
claim. Review the [raw evidence](evidence/phase2-2026-09-10.json) and ADR-0012
before drawing or publishing any comparison. Gitea
[run 6578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/6578)
recomputed it on evidence SHA `60a860995584334ecaa38d281b48499b6caaddc4`.

## Phase 3 Google Takeout plan

`scripts/benchmark-phase3.py` measures the public immich-rs Takeout planner and
the pinned immich-go dry-run against the same deterministic two-part ZIP view,
one worker and one loopback mock environment. Each pair rematerializes the
versioned synthetic corpus; fixture generation, oracle verification and mock
startup are outside the measured child-process interval. Order alternates
after two warmups across six retained pairs.

The exact implementation is
`39876419b01359103f05add0108afb600fcc5fa0`. Every sample observed four
physical media paths, five sidecars, three logical assets and zero retry. The
oracle made 17 mock requests, including the five contained job-resume PUTs;
immich-rs constructed no HTTP capability. Raw aggregates are:

| Metric | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.005121 | 0.006169 | 0.013780 | 0.018062 |
| User CPU (s) | 0.000000 | 0.003720 | 0.004292 | 0.019097 |
| System CPU (s) | 0.004627 | 0.008522 | 0.009827 | 0.014333 |
| Peak RSS (bytes) | 4,141,056 | 5,201,920 | 13,369,344 | 13,901,824 |
| Peak file descriptors | 4.5 | 7 | 12 | 13 |
| Characters read | 10,189.5 | 13,528 | 47,578.5 | 48,020 |
| Characters written | 0 | 4,486 | 13,952 | 14,657 |
| Storage bytes read | 0 | 0 | 0 | 0 |
| Storage bytes written | 0 | 0 | 0 | 0 |

This 1,296-byte corpus is a compatibility and harness reproducibility check,
not a throughput or scale benchmark. The values do not support a generalized
performance claim. Review the [raw evidence](evidence/phase3-2026-08-15.json)
and ADR-0012 before making any comparison.

Verification record: Gitea run 5264 repeated six paired samples after two
warmups on implementation SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933` and uploaded the raw report.

## Phase 4 Apple Photos plan

`scripts/benchmark-phase4.py` measures the public read-only Apple planner and
the pinned oracle dry-run on the same two deterministic iCloud-style ZIP
parts. The logical corpus is five assets, one XMP sidecar and 426 source bytes.
Both tools use concurrency one and the same loopback mock; order alternates
after two warmups across six retained pairs. Archive materialization, oracle
verification and mock startup are outside the measured interval.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.002867 | 0.009472 | 0.014914 | 0.031508 |
| Peak file descriptors | 4 | 6 | 14 | 14 |

The processes are too short and the corpus too small for a performance claim;
the report explicitly records raw measurements only. Its purpose is to prove
the paired harness, asset/sidecar counts, contained oracle mutations and exact
fixture identities. See the [raw evidence](evidence/phase4-2026-08-21.json).
Implementation SHA `b55132625ab6b1effbb07e42e79b357ae7c7de3f` and evidence
SHA `e02222bfdb253dfc3d2e1f5b6a61d2226c639f83` are green in Gitea run 5373.

## Phase 5 read-only archive

`scripts/benchmark-phase5.py` measures inventory plus original-byte archive
for immich-rs and immich-go on the same owner, four standalone originals and
disposable Immich v3.1.0 server. The logical payload is exactly 587,015 bytes.
Both tools use concurrency one and a warm cache. Owner/server setup, fixture
upload and post-run byte verification are outside the measured interval. The
complete public command workflow is timed, including two immich-rs child
commands (plan plus apply) and the oracle's single archive command. Execution
order alternates after two warmups across six retained pairs.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.040964 | 0.055234 | 0.093808 | 0.101784 |
| User CPU (s) | 0.004252 | 0.015031 | 0.008831 | 0.019603 |
| System CPU (s) | 0.008232 | 0.010629 | 0.008692 | 0.011583 |
| Peak RSS (bytes) | 6,787,072 | 7,065,600 | 15,642,624 | 16,613,376 |
| Peak file descriptors | 7.5 | 8 | 12 | 13 |
| Logical bytes read | 587,015 | 587,015 | 587,015 | 587,015 |
| Logical bytes written | 587,015 | 587,015 | 587,015 | 587,015 |

Median wall time is 56.3% lower for immich-rs, and its slowest retained sample
(55.234 ms) is below the oracle's fastest (77.248 ms). This statement applies
only to this exact small synthetic, warm-cache, loopback archive. It is not a
large-library, cold-storage, WAN or production claim. Review the
[raw evidence](evidence/phase5-2026-08-22.json), which binds both binaries,
fixture, server methodology, raw samples and exact source revision.

The companion [disposable evidence](../docs/evidence/phase5-disposable-archive-2026-08-22.json)
proves four first-run downloads, four verified resume hits, byte-multiset
equality and zero remaining labelled containers, volumes or networks. Push CI
recalculates every aggregate and rejects unsupported claim text.

## Phase 8 Google Takeout import

`scripts/benchmark-phase8.py` measures each tool's complete public plan plus
import workflow against the same 67,108,864-byte, eight-asset synthetic
Takeout tree. Every run receives a fresh isolated owner on one disposable
Immich v3.1.0 HTTPS server. The conventional JSON sidecars produce the same
observable result for both tools: eight visible assets, eight metadata
assignments and zero retries. Account creation, server startup and
postcondition probes are outside the measured interval.

Both tools use concurrency one. Execution order alternates after two warmups
across six retained pairs. The nearest-rank p95 is therefore the largest
retained sample. Process measurements include wall/user/system time, peak RSS,
peak descriptors and `/proc` character/storage I/O; server operation counters
are recorded separately.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 1.046122 | 1.412632 | 38.798523 | 39.092057 |
| User CPU (s) | 0.229150 | 0.304348 | 0.130493 | 0.233309 |
| System CPU (s) | 0.073241 | 0.232233 | 0.116262 | 0.123635 |
| Peak RSS (bytes) | 11,831,296 | 12,124,160 | 17,508,352 | 17,997,824 |
| Peak file descriptors | 14 | 14 | 17 | 17 |

Median wall time is 97.3% lower for immich-rs, and its slowest retained sample
(1.413 s) is below the oracle's fastest (38.618 s). This statement applies
only to this exact small synthetic, warm-cache, loopback import. It is not a
large-library, cold-storage, WAN or production claim. The
[raw evidence](evidence/phase8-2026-09-10.json) binds both binary digests,
fixture and corpus identities, source revision, environment, raw samples and
the exact permitted claim text.

The comparison ran on SHA `03e6e13855ad4b401143653c30fa352fc8761546`
and proved zero disposable containers, volumes, networks and staging residue.
Gitea [run 6578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/6578)
validated the evidence at `60a860995584334ecaa38d281b48499b6caaddc4`.
Push CI recomputes every aggregate and rejects a changed fixture, oracle digest,
operation count or unsupported performance claim.

## Phase 9 Apple Photos import

`scripts/benchmark-source-import.py` measures complete immutable planning plus
apply for immich-rs and the public immich-go `upload from-icloud` command. Both
tools receive a fresh owner on the same disposable Immich v3.1.0 server and the
same 67,108,864-byte, eight-asset upload-only compatibility intersection.
Concurrency is one, execution order alternates after two warmups and six pairs
are retained. Server/account setup and postcondition probes are excluded.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.729086 | 0.769765 | 33.788357 | 93.021256 |
| User CPU (s) | 0.176117 | 0.203603 | 0.116218 | 0.124090 |
| System CPU (s) | 0.093473 | 0.112809 | 0.105796 | 0.113330 |
| Peak RSS (bytes) | 11,094,016 | 11,403,264 | 16,535,552 | 16,592,896 |
| Peak file descriptors | 11 | 11 | 19 | 20 |

Median wall time is 97.8% lower for immich-rs, and its slowest retained sample
(0.770 s) is below the oracle's fastest (24.705 s). This satisfies ADR-0012
only for this exact small synthetic, warm-cache, loopback Apple import. It is
not a large-library, WAN, storage or production claim. The
[raw evidence](evidence/phase9-source-import-benchmark-2026-08-24.json) binds
all samples, identities, operations and the permitted claim text.

## Phase 10 Picasa import

The Picasa comparison uses the same harness, server, 64 MiB/eight-asset
upload-only intersection, fresh-owner isolation and alternating one-worker
methodology as Phase 9. Picasa album and caption extensions are intentionally
disabled because immich-go v0.32.0 does not expose the same observable surface.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.724882 | 0.783389 | 87.063794 | 99.286263 |
| User CPU (s) | 0.181498 | 0.222404 | 0.081423 | 0.099788 |
| System CPU (s) | 0.070181 | 0.104038 | 0.083799 | 0.120155 |
| Peak RSS (bytes) | 10,936,320 | 11,202,560 | 15,845,376 | 16,539,648 |
| Peak file descriptors | 11 | 11 | 14 | 14 |

Median wall time is 99.2% lower for immich-rs, and its slowest retained sample
(0.783 s) is below the oracle's fastest (24.581 s). This satisfies ADR-0012
only for the exact synthetic compatibility intersection; it is not a private
export, large-library, WAN or production claim. Review the
[raw evidence](evidence/phase10-source-import-benchmark-2026-08-24.json) and
its operation counters before publishing any comparison.

## Phase 11 Immich-to-Immich migration

`scripts/benchmark-real-migration.py` gives each tool fresh isolated source and
destination owners on the same two disposable Immich v3.1.0 servers. Every
source contains the same eight assets and 67,108,864 bytes. The measured scope
is complete inventory, immutable plan and migration for immich-rs versus the
public immich-go `upload from-immich` command; seeding, owner setup and outcome
probes are excluded. Both tools use concurrency one, alternate order after two
warmups and retain six pairs.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 1.394889 | 1.641281 | 124.048386 | 128.529436 |
| User CPU (s) | 0.291948 | 0.414094 | 0.194051 | 0.225151 |
| System CPU (s) | 0.249503 | 0.301722 | 0.224009 | 0.318159 |
| Peak RSS (bytes) | 13,271,040 | 14,184,448 | 17,096,704 | 19,091,456 |
| Peak file descriptors | 12 | 12 | 20 | 22 |

Median wall time is 98.9% lower for immich-rs, and its slowest retained sample
(1.641 s) is below the oracle's fastest (106.151 s). This satisfies ADR-0012
only for this exact small synthetic, loopback, two-server migration. It is not
a large-library, WAN or production claim, and production migration remains
unauthorized. The
[raw evidence](evidence/phase11-real-2026-08-24.json) binds all samples and
Gitea [run 5678](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5678)
to source SHA `88d9a59d1ca1d0cd92b3ef28349ff4b95a5f8095`.

## Authorized private Takeout shadow

An explicitly authorized private Google Photos Takeout was used only for an
immich-rs read-only scale sanity check. The runner selected one complete,
bounded top-level group, streamed its extraction on the NAS and committed only
aggregate counters. The selected view contained 1,778 files, 889 media
candidates and 455,403,635 bytes. All three retained runs produced 889 assets,
889 sidecars and the same byte count after one warmup.

Median wall time was 0.468807 seconds, peak client RSS was 8,466,432 bytes and
peak open file descriptors were 7. This is not a public benchmark corpus and
has no immich-go comparison, so it supports boundedness and determinism only.
No path, filename, metadata, media digest or content was committed. The
extracted subset and remote runner were removed, as recorded by the redacted
[aggregate evidence](../docs/evidence/phase6-private-takeout-shadow-2026-08-22.json).

## Phase 6 large synthetic soak

`scripts/run-synthetic-soak.py` creates an actually allocated, deterministic
Google Takeout tree, runs one warmup plus three retained immich-rs plans and
removes the exact temporary tree on every exit. The corpus contains 2,500
media files, 2,500 matching JSON sidecars and 1,311,002,500 logical source
bytes. Media generation and hashing use a 64 KiB buffer; the planner uses the
same bound.

All retained runs emitted the same normalized-plan digest and exact counters.
Wall times were 1.236492, 1.276256 and 1.274296 seconds; the median was
1.274296 seconds. Peak client RSS was 13,975,552 bytes and peak open file
descriptors were 7. These are warm page-cache boundedness and determinism
measurements, not cold-storage throughput or an immich-go comparison.

The [aggregate and raw process evidence](../docs/evidence/phase6-synthetic-soak-2026-08-22.json)
binds the deterministic generator, fully allocated byte count, corpus and plan
digests, exact implementation/binary, methodology and cleanup proof. Push CI
recalculates all aggregates and enforces the 256 MiB client RSS budget.
