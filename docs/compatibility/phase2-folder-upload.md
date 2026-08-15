# Phase 2 folder-upload gate matrix

This matrix describes the first mutation vertical. It does not extend the
Phase 1 semantic differential. Upload correctness is verified against the
synthetic mock and an isolated Immich v3.1.0 instance. immich-go remains a
black box and participates only in the derived standalone performance matrix.

| Surface | Mock | Disposable Immich | Declared outcome |
|---|---:|---:|---|
| Exact 3.1.0 release negotiation | Pass | Pass | Authenticated probe grants an opaque upload capability. |
| Missing authentication | Pass | Not exercised | Fails before any network request. |
| Incompatible version | Pass | Not exercised | Non-3.1 releases fail closed. |
| Oversized API response | Pass | Not exercised | Bounded response parsing fails closed. |
| Standalone image | Pass | Pass | One source operation creates one asset. |
| Standalone video | Pass | Pass | Streams as one independent operation. |
| XMP sidecar | Pass | Pass | Streams with its parent media operation. |
| Image/MOV live photo | Pass | Pass | Video completes before the dependent image. |
| Generic JSON sidecar | Unit pass | Not exercised | Explicitly rejected until Phase 3 reconciliation. |
| Dry-run | Pass | Pass | No API key, network request or checkpoint write. |
| Checkpoint resume | Pass | Pass | Completed operations return as resumed. |
| Fresh-checkpoint duplicate | Pass | Pass | Server checksum state converges without a new asset. |
| HTTP 429 | Pass | Not exercised | One bounded retry converges once. |
| HTTP 5xx | Pass | Not exercised | One bounded retry converges once. |
| Disconnect | Pass | Not exercised | One bounded retry converges once. |
| Commit with lost response | Pass | Not exercised | Checksum reconciliation prevents a second asset. |
| SIGINT cancellation and resume | Pass | Not exercised | Exit 130 leaves recoverable state and resumes once. |

The mock matrix uses only generated files and an in-process loopback server.
The real disposable gate uses an offline, byte-reproducible CC0 corpus with a
JPEG/XMP operation, a standalone MP4 and a JPEG/MOV live-photo operation. It
uses synthetic identity and credentials, exact container-image digests, a
dedicated non-masquerading Docker bridge and an ephemeral `127.0.0.1` port. It
created four operations, resumed four from the checkpoint, converged four as
duplicates from a fresh checkpoint and observed three visible assets with one
live-photo link.

The committed [matrix evidence](../evidence/phase2-disposable-matrix-2026-08-15.json)
records implementation SHA `3ed13d3293baf197fa5a21e624a828c201d7b763`.
Gitea run 5234 found zero labelled containers, volumes and networks after the
run and removed all three images it had downloaded.

The paired performance view replaces the two live-photo identifiers with
distinct fixed-size synthetic identifiers in a bounded stream. Across six
recorded pairs after two warmups, each tool produced four visible standalone
assets, zero live links and zero retries. The
[raw report](../../benchmarks/evidence/phase2-2026-08-15.json) is comparable
only for this declared corpus and makes no generalized performance or
production-authorization claim.
