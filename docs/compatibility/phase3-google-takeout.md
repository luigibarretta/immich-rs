# Phase 3 Google Takeout compatibility matrix

This first Phase 3 slice is enforced by
`takeout-differential-expectation-v1` against the exact immich-go v0.32.0
binary and the loopback-only synthetic Immich mock. It accepts only a
decompressed export root containing real `Takeout/Google Photos` directories.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Decompressed layout | Parity | Both implementations discover and retain the same two generated PNG paths. |
| JSON `title` matching | Parity | Both discover the same two generated JSON sidecars; immich-rs records `META_GOOGLE_TAKEOUT_TITLE_V1`. |
| Metadata association | Parity | The oracle metadata-updated paths equal the two plan assets with metadata candidates. |
| Dry-run mutations | Contained oracle defect | v0.32.0 sends the five pinned job-resume PUTs; no asset mutation is accepted. |

The adapter reads media one file at a time through the existing bounded hash
buffer. Each JSON sidecar is independently limited to 256 KiB and its bytes
are compared with the discovery identity before parsing. Malformed,
oversized, ambiguous and changed JSON becomes a stable error. Unsupported
metadata and media without JSON become stable warnings rather than silent
partial compatibility. Unchanged content is byte-deterministic across file
creation order, and cancellation returns no partial plan.

The following are deliberately unsupported in this slice: ZIP and split
archives, supplemental-metadata filename truncation, albums, descriptions,
coordinates, timestamps, timezone conversion, people and partner metadata.
No Takeout plan can enter the Phase 2 folder upload planner or executor.

The normalized local differential contains two planned assets, two oracle
unique assets, two sidecars and five contained oracle-defect mutations. All
four declared assertions pass. This small corpus is compatibility evidence,
not a performance benchmark or a generalized Phase 3 completion claim.
Gitea push CI run 5240 reproduced the matrix on implementation SHA
`a31c30714d879e5b63996d5ce258d70761f799ad`.
