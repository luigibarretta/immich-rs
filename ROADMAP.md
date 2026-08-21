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
records raw results and methodology. The current implementation was revalidated
with six paired samples after two warmups on SHA
`61383378f1db0999d463fe4180c156668ea0e2b6`: median folder scan/plan wall time
was 14.3% lower and p95 was 11.9% lower than immich-go v0.32.0 on the exact
64 MiB synthetic corpus. This is a scoped CPU-stage claim, not an upload,
Takeout or generalized speedup. Evidence commit
`8471ee49c4204bb5e2749f42381ef7f0fecc0448` is green in Gitea run 5356.

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

The complete read-only planner is implemented and evidence-enforced:

- one decompressed root or one to 64 independent split ZIP files, with no
  implicit extraction and no semantic input-order dependency;
- `normalized-plan-v2` source-neutral descriptions, canonical UTC timestamps,
  canonical coordinates and sorted album membership;
- deterministic title and supplemental filename reconciliation;
- content-identical alias collapse with year-folder canonical preference and
  preserved album evidence;
- fail-closed conflicting paths, metadata, encryption, unsafe paths,
  unsupported compression, suspicious ratios, CRC/read failures and source
  changes;
- exact directory/ZIP golden parity, order/buffer property tests, clean archive
  cancellation and the nine-row black-box compatibility matrix;
- six paired raw samples after two warmups over the same deterministic split
  corpus and environment;
- no Takeout upload, album creation or other mutation capability.

The local differential observes three normalized logical assets, four oracle
physical assets, two oracle dry-run uploads, five sidecars and only the five
contained job-resume PUTs. The supplemental filename behavior is an explicit
immich-go v0.32.0 divergence: the oracle classifies that sidecar as album
metadata and leaves its media pending. The committed 1,296-byte benchmark is
harness reproducibility evidence, not a throughput or performance claim.

Exit gate: the declared matrix, dependency boundary, deterministic tests,
bounded streaming contract and raw benchmark evidence are complete. Apply and
all Takeout mutations remain outside Phase 3. Gitea push CI run 5263 and manual
paired-benchmark run 5264 are green for exact implementation SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933`.

## Phase 4 — iCloud and Photos exports

The complete read-only Apple Photos planner is implemented and
evidence-enforced:

- one directory or up to 64 independent iCloud-style ZIP parts, streamed
  without extraction;
- XMP association, Live Photo pairing, Unicode NFC and known-export-noise
  diagnostics;
- lossless preserve-all handling for edited/original variants;
- explicit none, immediate-folder or full-path album derivation;
- deterministic `normalized-plan-v3`, exact directory/ZIP goldens, property
  tests, cancellation and the eight-row black-box compatibility matrix;
- six paired raw samples after two warmups on the same 426-byte synthetic
  corpus and loopback mock;
- no Immich client, upload or mutation capability in the dependency graph.

Exit gate: all declared compatibility rows pass. The tiny paired corpus is
reproducibility evidence and makes no performance claim. Implementation SHA
`a1e4f5d5c62aa8f336fe8773d5e7c6f0f404f453`, benchmark SHA
`b55132625ab6b1effbb07e42e79b357ae7c7de3f` and evidence SHA
`e02222bfdb253dfc3d2e1f5b6a61d2226c639f83` are green in Gitea run 5373.

## Phase 5 — archive and maintenance commands

The first read-only archive vertical is implemented and evidence-enforced:

- bounded, paginated timeline/archive/hidden/trash inventory from loopback-only
  Immich v3.1 with linked Live Photo original discovery;
- immutable `archive-manifest-v1` with exact IDs, safe names, sizes, SHA-1 and
  server/configuration binding;
- bounded original streaming, sibling partial files, checksum/length
  verification, flush and atomic rename;
- idempotent existing-file verification, local conflict exit class 9, clean
  cancellation and bounded retry/fault recovery;
- synthetic mock coverage plus a real disposable Immich canary with exact byte
  multiset verification and complete resource cleanup;
- six paired raw archive samples after two warmups against immich-go v0.32.0
  on the same owner, originals, server, warm cache and concurrency one;
- dependency enforcement proving that the archive path cannot construct a
  server mutation capability.

Exit gate: the synthetic disposable archive downloaded four originals, then
verified four and downloaded zero on rerun. On this exact 587,015-byte
four-asset corpus, immich-rs median wall time was 40.964 ms versus 93.808 ms
for immich-go, 56.3% lower; this is not a large-library or production claim.
Implementation and benchmark evidence are bound to SHA
`d3039c908d22249eeb6eb7c4c96500d80e1d6e04`. Evidence enforcement was added
in `d3229cb1f2917a3a806f864a6d30f8fd4dd98930`.

Replacement, delete, metadata mutation and maintenance writes remain absent.
They require explicit mutation gates and are not prerequisites for this
read-only Phase 5 vertical.

## Phase 6 — release candidate

- Linux x86-64, Linux arm64, macOS arm64/x86-64 and Windows x86-64 artifacts;
- signed checksums, SBOM and provenance;
- migration guide and rollback to immich-go;
- soak against a large synthetic library and an explicitly authorized,
  read-only production shadow run.

The scale gates are complete. A reproducible, fully allocated synthetic soak
planned 2,500 media plus 2,500 sidecars and 1,311,002,500 bytes identically in
three retained runs, with a 13,975,552-byte client RSS peak and seven file
descriptors. An explicitly authorized private Google Photos Takeout shadow
planned 889 media plus 889 sidecars and 455,403,635 bytes deterministically,
with an 8,466,432-byte client RSS peak. Only aggregate private counters are
committed and every temporary local/NAS resource was removed. These are
scale/boundedness results, not public performance comparisons.

No replacement of immich-go is considered before Phase 6 evidence.
