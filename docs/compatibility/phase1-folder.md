# Phase 1 folder compatibility matrix

The matrix is enforced by `differential-expectation-v1` against the exact
immich-go v0.32.0 binary and a loopback-only synthetic Immich mock. “Parity”
means the normalized observable outcome matches; it does not imply identical
internal algorithms or CLI prose.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Recursive regular media | Parity | The eight unique oracle uploads equal the eight golden plan assets. |
| Unicode | Parity after normalization | Oracle paths and plan paths compare in NFC. |
| Case collision | Parity with an immich-rs diagnostic | Both case-distinct assets remain; immich-rs adds `FS_CASE_COLLISION_V1`. |
| Duplicate basename | Parity with an immich-rs diagnostic | Both directory-qualified assets remain; immich-rs adds a warning. |
| XMP sidecar | Parity | Both discover the same XMP candidate. |
| Generic JSON sidecar | Declared oracle divergence | immich-rs retains a candidate; the oracle rejects the synthetic JSON as non-immich-go metadata. |
| Live-photo image/MOV | Parity | The oracle stacked paths equal the two plan members. |
| Symlink | Equivalent unique plan | immich-rs does not follow it; the oracle discovers then discards it as a local duplicate. |
| Dry-run mutations | Contained oracle defect | v0.32.0 sends five job-resume PUTs; the case pins them exactly and permits no asset mutation. |

Unreadable files, path limits, NFC collisions, source changes and cancellation
have deterministic immich-rs contract tests but no stable black-box oracle
observable in this matrix. They are not silently counted as parity.

The normalized local observation contains 9 discovered oracle assets, 8 unique
oracle assets, 8 planned immich-rs assets and 3 distinct sidecar paths. Every
matrix assertion passes with the pinned baseline and synthetic corpus. Gitea
push CI run 5165 repeated the differential successfully on implementation SHA
`36d0f7f55308e1b578474ae0bec9346e27ea0365`.
