# Delivery roadmap

No phase advances because code exists. It advances only when its acceptance
evidence is committed and CI-enforced.

## Phase 0 — architecture and harness

- accepted ADR set;
- buildable workspace and strict CI;
- synthetic fixture format and redaction check;
- pinned `immich-go` oracle runner;
- benchmark harness that records wall time, CPU time, peak RSS, bytes read and
  bytes written;
- mock Immich server capable of recording requests and injecting failures.

Exit gate: identical normalized plans for the first folder fixtures, with no
production endpoint or credential involved.

## Phase 1 — read-only scan and plan

- recursive folder enumeration;
- content identity and sidecar discovery;
- deterministic normalized plan output;
- bounded memory and cancellation;
- no upload or server mutation.

Exit gate: golden and differential parity for supported folder cases, plus
published benchmark evidence.

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
