# Phase 3 Google Takeout compatibility matrix

The complete read-only surface is enforced by
`takeout-differential-expectation-v2` against the exact immich-go v0.32.0
binary and a loopback-only synthetic Immich mock. The fixture has equivalent
decompressed and deterministic two-part ZIP views. immich-rs never extracts
the archives and never constructs a network capability.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Split ZIP layout | Parity | Both tools discover the same four physical paths across both archive parts. |
| JSON `title` metadata | Parity | Both associate the alpha and NFC Unicode sidecars with their media. |
| Supplemental filename | Declared oracle divergence | immich-rs resolves `beta.png.supplemental-metadata.json`; the oracle classifies it as album metadata and leaves beta pending. |
| Content alias | Parity after logical deduplication | Both identify the album copy as a duplicate of alpha; immich-rs emits one logical asset. |
| Album membership | Parity after logical deduplication | Alpha retains `Synthetic Album`; the oracle reports three album additions over physical paths. |
| Description and location | Stronger normalized immich-rs contract | Every logical asset has a bounded description; alpha selects canonical `geoDataExif` coordinates. |
| UTC timestamp | Parity except supplemental divergence | Canonical UTC seconds match observable oracle dates; beta remains pending only because of the declared sidecar divergence. |
| Unicode NFC | Parity after normalization | The NFC `café.png` path is discovered and selected by both tools. |
| Dry-run mutations | Contained oracle defect | The five pinned job-resume PUTs are contained; no asset mutation is accepted. |

The normalized plan contains three logical assets from four physical media
paths and five sidecars. The oracle discovers four physical assets and selects
two for its dry-run upload because beta is left pending. Exact-content aliases
collapse in the plan, so physical counts are never presented as logical parity.

Archive input is limited to 64 non-symlink ZIP files and stored or DEFLATE
entries. Path safety, Unicode, encryption, entry count, declared size,
compression ratio, CRC/read integrity and source metadata changes fail closed.
Entry payloads are streamed through the configured bounded buffer one archive
and entry at a time; JSON has a separate 256 KiB ceiling. Directory and split
ZIP views produce the same golden `normalized-plan-v2`, independent of archive
input order, entry order and tested buffer size. Cancellation returns no
partial plan.

Descriptions, strict Unix-second capture time, finite in-range coordinates and
album names are normalized without locale or host-timezone input. Conflicting
logical paths, alias metadata or ambiguous sidecars remain explicit errors.
People, partner metadata, face state, archive/trash state and edited/original
policy are intentionally unsupported.

The paired benchmark uses this same 1,296-byte logical corpus, two warmups and
six alternating samples at concurrency one. It records raw wall time, CPU,
peak RSS, descriptors, procfs I/O, logical bytes, operations and retries. It is
a reproducibility check, not a throughput or generalized performance claim.
No Takeout plan can enter the Phase 2 upload planner or executor.

Gitea push CI run 5263 and manual paired-benchmark run 5264 reproduce this
matrix and benchmark on exact implementation SHA
`430e7fb95f11188c7c854721ef5ede19cbc2e933`.
