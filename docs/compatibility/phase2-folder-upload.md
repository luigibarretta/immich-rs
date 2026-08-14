# Phase 2 folder-upload gate matrix

This matrix describes the first mutation vertical. It does not extend the
Phase 1 immich-go differential: immich-go remains a black-box scan oracle, while
upload behavior is verified against the synthetic mock and an isolated Immich
v3.1.0 instance.

| Surface | Mock | Disposable Immich | Declared outcome |
|---|---:|---:|---|
| Exact 3.1.0 release negotiation | Pass | Pass | Authenticated probe grants an opaque upload capability. |
| Missing authentication | Pass | Not exercised | Fails before any network request. |
| Incompatible version | Pass | Not exercised | Non-3.1 releases fail closed. |
| Oversized API response | Pass | Not exercised | Bounded response parsing fails closed. |
| Standalone image | Pass | Pass | One source operation creates one asset. |
| Standalone video | Pass | Not exercised | Streams as one independent operation. |
| XMP sidecar | Pass | Not exercised | Streams with its parent media operation. |
| Image/MOV live photo | Pass | Not exercised | Video completes before the dependent image. |
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
The real disposable gate uses one generated CC0 PNG, synthetic identity and
credentials, exact container-image digests, a dedicated non-masquerading Docker
bridge and an ephemeral `127.0.0.1` port. It observed one asset after create,
checkpoint resume and a fresh-checkpoint duplicate check.

The committed [disposable evidence](../evidence/phase2-disposable-2026-08-15.json)
records implementation SHA `ae3cb5c7ecef046a7fb12c37c8ca2081e1887cf3`.
Post-run inspection found zero labelled containers, volumes and networks; all
three images pulled by that run were removed. This is compatibility and safety
evidence, not a Phase 2 performance comparison or production authorization.
