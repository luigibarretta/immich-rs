# Delivery roadmap

No phase advances because code exists. It advances only when its acceptance
evidence is committed and CI-enforced.

## Phase 0 — architecture and harness

Complete and CI-enforced on implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`:

- accepted architecture and Phase 0 contract ADRs;
- buildable workspace, dependency policy and 400-LOC source guard;
- versioned synthetic fixture and expected-plan formats;
- fail-closed credential, production-host, personal-metadata and provenance
  checks;
- pinned, digest-verified black-box immich-go runner with bounded capture;
- loopback mock Immich server with request recording and fault injection;
- versioned source-neutral normalized plan, diagnostics, progress and
  cancellation contracts;
- paired benchmark metrics and reproducibility manifest;
- fast push CI plus a separate manual full-benchmark workflow.

Exit evidence: synthetic-only fixtures, exact oracle identity, stable normalized
plans, deterministic tests and no production access. Gitea push CI run 5165 is
green for the exact implementation SHA.

## Phase 1 — read-only scan and plan

Complete and CI-enforced on implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`:

- recursive deterministic folder enumeration;
- streaming content identity with bounded memory and file descriptors;
- explicit regular-file, symlink, Unicode/NFC collision, case collision,
  duplicate basename, unreadable-file, path-limit and source-change behavior;
- deterministic JSON/XMP sidecars and live-photo pairing;
- read-only `normalized-plan-v1` output only;
- exact golden, property, cancellation and black-box differential tests;
- enforced read-only dependency graph with no Immich client capability;
- six paired raw benchmark samples after two warmups on the same 64 MiB
  synthetic corpus and environment.

Exit evidence: all declared compatibility rows pass, including documented
intentional divergence and oracle-defect containment. The committed benchmark
records raw results and methodology without claiming a generalized speedup.
Gitea push CI run 5165 and manual paired-benchmark run 5170 are green for the
exact implementation SHA; run 5170 uploaded the raw JSON report.

## Phase 2 — folder upload MVP

- version-negotiated Immich client;
- streaming multipart upload;
- duplicate handling;
- bounded concurrency, retries and resumable checkpoint journal;
- explicit dry-run and plan/apply separation.

Exit gate: isolated disposable Immich test instance, fault-injection recovery,
idempotent second run and no duplicate assets.

## Phase 3 — Google Takeout

- multi-archive virtual input;
- JSON sidecar matching and filename/time normalization;
- albums, descriptions, locations and timezone behavior;
- adversarial split/duplicate metadata fixtures.

Exit gate: declared compatibility matrix fully green against the oracle.

## Phase 4 — iCloud and Photos exports

- iCloud archive/folder layouts;
- XMP and live-photo pairing;
- edited/original asset policy;
- albums and metadata parity.

## Phase 5 — archive and maintenance commands

- read-only archive from Immich;
- replacement and maintenance operations only behind explicit mutation gates;
- stable machine-readable reports and exit codes.

## Phase 6 — release candidate

- Linux x86-64, Linux arm64, macOS arm64/x86-64 and Windows x86-64 artifacts;
- signed checksums, SBOM and provenance;
- migration guide and rollback to immich-go;
- soak against a large synthetic library and an explicitly authorized,
  read-only production shadow run.

No replacement of immich-go is considered before Phase 6 evidence.
