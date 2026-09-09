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

The migration/rollback guide and fail-closed release pipeline are implemented.
The pipeline requires an annotated signed RC tag, a clean exact revision,
native tests/builds for all five ADR-0014 targets, CycloneDX 1.5 SBOMs,
deterministic archives, redacted provenance, a complete SHA-256 manifest and a
verified OpenPGP signature. Push CI enforces this contract.

Secondary container packaging is implemented under ADR-0026: a digest-pinned,
non-root, shell-free OCI image, hardened offline Compose service, strict
environment/TOML configuration and a separate attested `linux/amd64` plus
`linux/arm64` build. The RC workflow includes the verified OCI archive, SPDX
SBOM and SLSA provenance in the signed checksum bundle. This does not replace
any native target or signing prerequisite.

The macOS ARM64 native prerequisite is complete. Repository runner ID 2,
`immich-rs-macos-arm64`, executed manual Gitea run 5440 on exact SHA
`1168b17aa8349f76064e80471c0b72bf55009978`: 64 Python tooling tests and 76
Rust tests passed, Clippy denied warnings, the release build succeeded and the
binary was verified as Mach-O ARM64. The non-root host runner was stopped after
the run and is intentionally offline when not in use.

The Phase 6 release-candidate gate is **passed** for version `0.1.0-rc.1`.
GitHub native-matrix run 33849192526 built and tested Linux x86-64/ARM64,
macOS x86-64/ARM64 and Windows x86-64 on exact revision
`f69a4c09bdeb09fbdae6d61260b0f8a20e2b6c3c`; Gitea run 6155 passed on that same
revision. The committed OpenPGP public key matches the independently encrypted
Vault identity, and the protected GitHub release environment contains only the
private key, fingerprint and passphrase required by ADR-0025. The signed tag
still remains fail-closed until the exact version commit also passes both main
pipelines.

No replacement of immich-go is considered before Phase 6 evidence.

## Phase 7 — production transport and authorization

ADR-0027 defines the first production-capable surface without authorizing a
real production test. The implementation must retain every immutable-plan,
bounded-streaming, checkpoint, retry and duplicate guarantee already proven by
the disposable folder upload and read-only archive verticals.

Acceptance requires:

- separate disposable-loopback and production-HTTPS endpoint capabilities;
- CLI-only remote read and write acknowledgements that cannot persist in
  environment, TOML, Compose or checkpoints;
- production write binding to the exact upload-plan SHA-256, expected operation
  count and a redacted verified-backup reference;
- remote read capability that cannot become a mutation capability;
- a synthetic disposable Immich behind a dedicated trusted HTTPS origin;
- certificate, authentication, version, retry, disconnect, lost-response,
  cancellation, resume and duplicate-convergence coverage;
- a bounded end-to-end mutating soak with complete disposable cleanup;
- no production hostname, credential, media or scheduled production mutation;
- unchanged immich-go periodic homelab smoke and an explicit rollback path.

The gate is complete on implementation SHA
`db6ec2185b0e2bff8be8cb017f0fe8fafba95eb8`. Push CI run 5539 and disposable
HTTPS run 5544 are green on that exact SHA. The synthetic gate proved private
CA and hostname verification, refusal classes, bounded recovery for 429, 5xx,
disconnect and a lost response after commit, cancellation/resume, four creates,
four checkpoint resumes, four fresh-checkpoint duplicates and four archived
originals. Cleanup left zero labelled containers, volumes and networks and no
temporary key or credential. The committed evidence on SHA
`1d0a7199a0604fec42abbaee711e5aca8fb2ebda` is green in push CI run 5547.

Production capability remains deliberately narrow: remote read-only archive
and immutable folder upload only, against declared Immich v3.1.x servers.
Google Takeout apply is completed in Phase 8. Apple Photos apply, release
publication and public forge visibility remain separate later verticals.

## Phase 8 — source-aware imports

ADR-0028 defines Google Takeout import as a distinct capability rather than an
extension of folder upload. Phase 8 is **complete**:

- `upload-plan-v2` is schema- and source-bound to Google Takeout or Apple
  Photos and cannot be consumed as `upload-plan-v1`;
- decompressed and split-ZIP Takeout inputs retain bounded source locators plus
  streaming SHA-256 and base64 SHA-1 identities;
- normalized timestamps, descriptions, locations and sorted albums enter the
  immutable plan;
- the summary exposes uploads, metadata updates, maximum album creates,
  deterministic membership requests and their maximum mutation sum;
- source-aware dry-run verifies the input offline without a credential,
  checkpoint or network capability;
- apply stages at most one archive media entry and optional XMP, verifies exact
  source facts and removes plan-bound staging on every outcome;
- a distinct import client converges upload, normalized metadata, exact album
  creation and membership in order, with bounded retry and lost-response
  reconciliation;
- effect-level `checkpoint-v2` resumes committed work, while plan digest,
  maximum mutation count, server and hashed backup reference remain immutable
  production authorization inputs;
- disposable Immich v3.1.0 proves three uploads, three metadata assignments,
  one album, one membership, resume, fresh-checkpoint duplicate convergence
  and exact zero-resource cleanup;
- the paired benchmark proves exact 8-asset/8-metadata/0-retry postcondition
  parity on the same 64 MiB synthetic corpus and environment.

The production authorization implementation is green in Gitea push CI
[run 5568](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5568)
on SHA `7387a260818e8f47cfe32b2f3b4eea81c90e2013`. The disposable gate is green in
[run 5570](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5570)
on SHA `5172c73fe615658d3a90d5d5849ca2209142db12`, with evidence enforcement green
in [run 5571](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5571).
The paired benchmark and cleanup gate are green in
[run 5578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5578)
on SHA `4edae0365ac5137a2ee99d216755d8e3693cf9c8`; raw evidence and aggregate
claims are committed and recalculated by push CI.

Apple Photos apply is implemented in the following separately gated vertical.
No delete, replace, trash, tag, people, stack or independent metadata/album
maintenance mutation is authorized.

## Phase 9 — Apple Photos import

ADR-0030 authorizes Apple Photos/iCloud import without changing the Phase 4
preserve-all scan contract. The implementation and synthetic compatibility
gate are complete:

- directory and bounded split-ZIP scans retain ephemeral native locators;
- `upload-plan-v2` binds Apple source configuration, transport timestamps,
  XMP, Live Photo roles and deterministic album effects;
- dry-run rescans offline without credentials, checkpoints or HTTP capability;
- apply selects the source adapter from the immutable plan and reuses bounded
  one-entry staging, capped retries and effect-level `checkpoint-v2` resume;
- source or configuration drift fails before mutation;
- no Apple JSON is interpreted as authorization for independent metadata
  mutation;
- disposable Immich v3.1.0 proves five assets, one Live Photo link, one
  five-member album, seven-effect resume and duplicate convergence;
- the pinned black-box `from-icloud` run has normalized asset, XMP, Live Photo,
  album and membership parity, while containing the oracle's five job writes;
- comparable paired raw measurements use the same 64 MiB/eight-asset corpus,
  server, concurrency and alternating order.

The synthetic Phase 9 gate is passed. The broader Phase 9 gate remains open
only for the aggregate-only personal Apple export shadow explicitly required by
ADR-0030. No private export has been supplied. On the exact synthetic 64 MiB
compatibility intersection, immich-rs median wall time is 97.8% lower and all
retained ranges are disjoint; this is not a private-export, WAN or production
claim.

## Phase 10 — Picasa import

ADR-0031 defines Picasa as a source contract rather than treating it as a
generic folder. Its implementation and synthetic compatibility gate are
complete:

- one directory or bounded split-ZIP set with deterministic source binding;
- bounded UTF-8 `.picasa.ini` album and per-file caption handling;
- optional filename-date fallback that never overrides stronger metadata;
- XMP, Live Photo, collision, cancellation and source-drift behavior inherited
  from the bounded source scanner;
- immutable `upload-plan-v2`, offline dry-run, one-entry staging and
  effect-level `checkpoint-v2` apply;
- four-asset real disposable apply, eight-effect resume, fresh-checkpoint
  duplicate convergence and zero-resource cleanup;
- black-box `from-picasa` asset/date/XMP/Live Photo parity, with caption and
  Picasa-album preservation declared as immich-rs extensions;
- comparable raw measurements on the same 64 MiB/eight-asset upload-only
  compatibility intersection.

The synthetic Phase 10 gate is passed. The authorized personal Picasa export
shadow required by ADR-0031 remains pending because no private export has been
supplied. On the exact synthetic 64 MiB compatibility intersection, immich-rs
median wall time is 99.2% lower and all retained ranges are disjoint; this is
not a private-export, WAN or production claim.

## Phase 11 — Immich-to-Immich migration

ADR-0032 defines a distinct two-server migration rather than composing archive
and folder upload manually. Its disposable compatibility gate is complete:

- `migration-plan-v1` binds distinct source and destination identities,
  bounded inventory and a maximum mutation budget;
- the source capability is read-only by type and source/destination credentials
  must be different;
- apply stages and verifies at most one original before destination upload;
- capture time, description, location and owned albums are preserved;
- effect-level checkpoint resume, fresh-checkpoint duplicate convergence,
  response-loss recovery and cancellation are bounded;
- two real Immich v3.1.0 stacks prove source immutability, three destination
  assets, metadata, album membership and exact zero-resource cleanup;
- the pinned `from-immich` oracle matches the declared originals, metadata and
  album surface; linked Live Photo motion preservation is an immich-rs
  extension;
- comparable real-server raw measurements use fresh owners, the same 64 MiB
  corpus, two servers, concurrency one and alternating order.

The disposable Phase 11 gate is passed. An explicitly authorized
non-production export shadow remains pending. Production migration is not
authorized by ADR-0032. On the exact synthetic 64 MiB two-server corpus,
immich-rs median wall time is 98.9% lower and all retained ranges are disjoint;
this is not a non-production shadow, WAN or production claim.

## Optional Web Console

ADR-0033 accepts a separate authenticated operator console without changing
the supported CLI or authorizing production migration. The application facade
and authenticated loopback folder scan/review library surface are implemented.
The remaining server, durable-state, apply, source-import, LAN, release and
observability slices stay unsupported until their evidence gates are green.

Sequential gates are:

1. **Implemented:** thin `immich-application` facade with byte- and
   exit-compatible CLI use;
2. **Implemented:** authenticated loopback folder scan/review with opaque
   configured profiles and hardened development-listener tests;
3. **Implemented:** bounded authenticated SSE and polling, isolated replay,
   cooperative cancellation and owned shutdown;
4. **Implemented:** private state-root, exact-host/CIDR-pinned server profile
   policy, separate bounded terminal-history persistence, server-bound folder
   probe/planning and immutable authenticated inspection/streaming export are
   implemented together with mandatory offline dry-run, atomic versioned
   receipts and source/profile/state drift refusal;
5. **Implemented:** exact single-use folder apply grants against disposable
   loopback, with private executor checkpoints, bounded replay/fault recovery
   and mandatory fresh dry-run confirmation for resume;
6. **Partially implemented:** Google Takeout, Apple Photos and Picasa
   source-only, server-bound plan and offline dry-run preserve their plan
   versions and bounded adapter options; exact-grant apply remains disabled
   until its separate gate;
7. TLS/OIDC LAN mode against a disposable identity provider;
8. separate five-target native artifact and hardened multiarch OCI service;
9. aggregate metrics, recovery/operator documentation, accessibility and
   browser regression closeout.

Production Immich-to-Immich migration, gallery/media serving, arbitrary path or
URL input, and delete/replace/trash/tag/people/stack/maintenance operations are
not part of this roadmap.
