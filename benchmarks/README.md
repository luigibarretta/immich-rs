# Phase 1 benchmark methodology

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
