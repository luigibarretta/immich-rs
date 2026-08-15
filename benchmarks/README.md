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

Verification record: Gitea run 5170 completed six paired samples after two
warmups on implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365` and uploaded
`phase1-benchmark-36d0f7f55308e1b578474ae0bec9346e27ea0365`. The report records
the 64 MiB, eight-asset fixture and the pinned immich-go v0.32.0 digest.

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
