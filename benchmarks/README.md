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
`61383378f1db0999d463fe4180c156668ea0e2b6` completed six paired samples
after two warmups. The committed
[raw report](evidence/phase1-2026-08-21.json) records the 64 MiB, eight-asset
fixture, both binary digests and the pinned immich-go v0.32.0 identity. Push CI
run 5356 validated that report and the full repository on evidence commit
`8471ee49c4204bb5e2749f42381ef7f0fecc0448`.

| Lower is better | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.049371 | 0.051393 | 0.057624 | 0.058321 |
| Peak RSS (bytes) | 4,642,816 | 4,964,352 | 16,072,704 | 18,317,312 |

The median wall-time reduction is 14.3%, the p95 reduction is 11.9% and the
measured median RSS reduction is 71.1%. The wall-time ranges do not overlap.
These results satisfy the ADR-0012 threshold for a scoped folder scan/plan
claim. They do not support an upload, Takeout, cold-storage or generalized
end-to-end speed claim.

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

Gitea run 5234 completed six recorded pairs after two warmups on implementation
SHA `3ed13d3293baf197fa5a21e624a828c201d7b763`. Every tool/sample produced four
visible assets, zero live-photo links and zero retries. These are the committed
raw aggregates:

| Metric | immich-rs median | immich-rs p95 | immich-go median | immich-go p95 |
|---|---:|---:|---:|---:|
| Wall time (s) | 0.310600 | 0.379116 | 61.367864 | 89.838749 |
| User CPU (s) | 0.010874 | 0.014248 | 0.025481 | 0.055146 |
| System CPU (s) | 0.011954 | 0.022542 | 0.032072 | 0.066544 |
| Peak RSS (bytes) | 8,214,528 | 8,429,568 | 15,915,008 | 17,928,192 |
| Peak file descriptors | 11 | 12 | 17.5 | 18 |
| Characters read | 2,399,588 | 2,399,595 | 1,252,687 | 1,253,337 |
| Characters written | 726,183 | 726,217 | 612,998.5 | 617,074 |
| Storage bytes read | 4,096 | 1,232,896 | 0 | 114,688 |
| Storage bytes written | 221,184 | 221,184 | 71,680 | 98,304 |

The wall-time distribution is especially wide for immich-go on this tiny
corpus. The report therefore makes no performance-improvement claim. Review
the [raw evidence](evidence/phase2-2026-08-15.json) and ADR-0012 before drawing
or publishing any comparison.

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
