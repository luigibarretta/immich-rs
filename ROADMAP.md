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

- version-negotiated Immich client with separate probe and upload capabilities;
- bounded streaming multipart upload for image, video, XMP and live photo;
- checksum duplicate handling, including commit with a lost response;
- bounded concurrency, retries and a durable resumable checkpoint journal;
- explicit immutable plan, dry-run and apply separation;
- loopback-only disposable-server authorization boundary;
- synthetic mock coverage for retry, refusal and cancellation behavior;
- real Immich v3.1.0 create, resume and duplicate convergence evidence;
- offline, byte-reproducible image/XMP, video and Apple live-photo corpus;
- six paired raw upload samples against immich-go after two warmups on one
  server, environment and derived standalone corpus.

Exit gate: isolated disposable Immich test instance, fault-injection recovery,
idempotent second run and no duplicate assets. The folder-upload MVP gate is
complete on implementation SHA `3ed13d3293baf197fa5a21e624a828c201d7b763`:
Gitea run 5234 verified four operations, one linked live photo, six paired
samples and complete disposable cleanup, then published both raw reports. Push
CI run 5235 is green for evidence-enforcement SHA
`6975633920117948b709bd7e5170c1246bd73b89`.

The benchmark is evidence for this exact small synthetic corpus, not a
generalized performance claim. Production remains unauthorized, and the
read-only Phase 3 work does not change that boundary.

## Phase 3 — Google Takeout

The first read-only vertical is implemented on SHA
`a31c30714d879e5b63996d5ce258d70761f799ad`:

- one decompressed root containing `Takeout/Google Photos`;
- additive `google_takeout` source kind in `normalized-plan-v1`;
- 256 KiB bounded JSON parsing and deterministic same-directory `title`
  matching;
- explicit malformed, oversized, unsupported, ambiguous and unmatched-media
  diagnostics;
- exact golden, creation-order property, cancellation and black-box
  differential tests;
- four passing compatibility rows over two generated PNGs and two generated
  sidecars, with the oracle's five job-resume PUTs contained by the mock;
- no Takeout apply, upload or other mutation path.

Gitea push CI run 5240 is green for the exact implementation SHA and executes
the Takeout differential on every push.

This is not the full Phase 3 gate. Remaining work is:

- multi-archive virtual input;
- truncated/supplemental JSON filename and time normalization;
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
