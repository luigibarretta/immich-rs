# Apple Photos import compatibility matrix

This vertical turns the accepted read-only Apple Photos plan into a bounded
source-aware import. It does not add delete, replace, trash or an independent
metadata/album maintenance command.

| Surface | Current outcome | Evidence |
|---|---|---|
| Directory input | Implemented | A resolved scan retains native paths, byte lengths and streaming identities without serializing host paths. |
| Split ZIP input | Implemented | Independent ZIP parts retain bounded entry indexes and are staged one media entry plus optional XMP at a time. |
| Live Photos and XMP | Implemented | Immutable operations preserve paired roles and attach at most one reconciled XMP sidecar. |
| Album modes | Implemented | `none`, `folder` and `path` plus the bounded joiner are configuration-digested and drift fails closed. |
| Transport timestamps | Implemented | Filesystem and ZIP timestamp facts populate upload transport fields only; they never become normalized capture metadata. |
| Dry-run | Implemented | Exact source rescan is offline and creates neither credential capability nor checkpoint. |
| Apply and resume | Verified | The plan selects the Apple adapter and uses capped retry, one-entry staging and seven-effect `checkpoint-v2` resume. |
| Production authorization | Verified by the shared Phase 7/8 boundary | Exact plan digest, maximum mutation budget, server identity and hashed backup reference are required for remote HTTPS apply. |
| Disposable Immich postconditions | Verified | Immich v3.1.0 observed five assets, one Live Photo link and one five-member album; fresh-checkpoint duplicate convergence and zero-resource cleanup passed. |
| immich-go v0.32.0 `from-icloud` parity | Verified | Both black-box runs produced five assets, one album, five memberships, Live Photo and normalized XMP parity. The oracle's five job writes remain a contained defect. |
| Comparable benchmark | Verified; scoped claim | On the same 64 MiB/eight-asset upload-only corpus, server and one-worker alternating methodology, median wall time was 0.729 s for immich-rs and 33.788 s for immich-go; every retained range was disjoint. |
| Authorized personal export | Pending | Aggregate-only shadow evidence will be produced after the maintainer supplies an explicit export. |
| Unsupported mutations | Absent | No delete, replace, trash, tag, people, stack or independent maintenance command exists. |

The synthetic implementation and compatibility gate is complete. Its strict
aggregate report is committed in
[`phase9-source-import-2026-08-24.json`](../evidence/phase9-source-import-2026-08-24.json),
and the paired raw process report is committed in
[`phase9-source-import-benchmark-2026-08-24.json`](../../benchmarks/evidence/phase9-source-import-benchmark-2026-08-24.json).
Gitea [run 5675](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5675)
produced both reports on exact source SHA
`f8c13eb4d9c7cfab968b1355004ae403a146d0f9`.
Phase 9 as a whole remains open only for the explicitly authorized personal
Apple export shadow required by ADR-0030. No personal path, filename, metadata,
digest or media content may enter that future aggregate evidence.
