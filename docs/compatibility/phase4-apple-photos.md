# Phase 4 Apple Photos compatibility matrix

The Phase 4 read-only surface is enforced by
`apple-differential-expectation-v1` against the exact immich-go v0.32.0
binary and a loopback-only synthetic Immich mock. The same five logical media
assets and one XMP sidecar are represented as a directory and as two
deterministic iCloud-style ZIP parts. immich-rs never extracts the ZIPs and
never constructs an Immich client capability.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Split ZIP layout | Parity | Both tools discover all five assets across both archive parts. |
| Asset discovery | Parity | Planned immich-rs paths equal the oracle's discovered and dry-run upload paths. |
| XMP sidecar | Parity | Both associate the exact `pair.xmp` sidecar with the still image. |
| Live Photo | Parity | Both group the same image/MOV pair by unambiguous same-directory basename. |
| Edited/original policy | Preserve-all parity | Both retain the distinct rendered and rendered-edited assets; immich-rs never guesses a relation. |
| Known export noise | Parity after diagnostic normalization | `.DS_Store` and `Recently Deleted` are excluded and immich-rs emits bounded rule IDs. |
| Unicode NFC | Parity after normalization | The decomposed input name is emitted as the same NFC portable path. |
| Dry-run mutations | Contained oracle defect | The five pinned job-resume PUTs are contained; no asset mutation is accepted. |

Album derivation is an immich-rs plan contract rather than an oracle parity
claim. `none` is the default, `folder` uses the immediate parent, and `path`
uses the full normalized parent path with a bounded caller-selected joiner.
Root assets receive no derived album. Results are sorted and deduplicated.

Directory and ZIP inputs cannot be mixed. ZIP inputs are limited to 64 regular
files and stored or DEFLATE entries. Symlinks, traversal, encryption,
unsupported compression, suspicious ratios, excessive sizes, CRC/read errors,
conflicting virtual paths and source changes fail closed. Payloads are streamed
through the configured bounded buffer. Cancellation returns no partial plan.

The exact golden is `normalized-plan-v3`: five assets, one XMP sidecar and 426
source bytes. Property tests vary ZIP input order, entry order, buffer size and
album mode. The paired benchmark retains six alternating samples after two
warmups, but its 426-byte corpus is compatibility evidence only and makes no
performance-improvement claim.

Implementation SHA `a1e4f5d5c62aa8f336fe8773d5e7c6f0f404f453`, benchmark
SHA `b55132625ab6b1effbb07e42e79b357ae7c7de3f` and evidence SHA
`e02222bfdb253dfc3d2e1f5b6a61d2226c639f83` are green in Gitea run 5373.
No Apple Photos plan can enter an upload or mutation executor.
