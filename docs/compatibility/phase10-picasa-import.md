# Picasa import compatibility matrix

This vertical implements Picasa directory and split-ZIP import as a distinct
source contract. It never treats arbitrary INI fields as mutation authority.

| Surface | Current outcome | Evidence |
|---|---|---|
| Directory and split ZIP | Verified | Versioned synthetic views produce semantically equivalent operations; only source transport timestamps may differ. |
| `.picasa.ini` | Verified | Bounded UTF-8 parsing accepts a Picasa album name and per-file caption; conflicts and invalid values fail closed. |
| Filename date | Verified | The explicit fallback matches the black-box oracle and never overrides stronger metadata. |
| XMP and Live Photo | Verified | One XMP sidecar and one image/MOV pair are preserved with normalized parity. |
| Dry-run | Verified | Exact source verification reads no credential and creates no checkpoint or network capability. |
| Apply and resume | Verified | Four creates, two metadata assignments, one album batch and eight-effect resume passed. |
| Fresh-checkpoint convergence | Verified | Four existing assets, the album and its four memberships converge without duplicate assets. |
| immich-go v0.32.0 `from-picasa` | Declared parity plus extensions | Asset, filename-date, XMP and Live Photo behavior match. Caption preservation and Picasa album creation are explicit immich-rs extensions; five oracle job writes are contained. |
| Comparable benchmark | Verified; scoped claim | On the same 64 MiB/eight-asset upload-only intersection with one worker and alternating order, median wall time was 0.725 s for immich-rs and 87.064 s for immich-go; every retained range was disjoint. |
| Authorized personal export | Pending | ADR-0031 requires a separately authorized aggregate-only Picasa shadow. |
| Unsupported mutations | Absent | Tags, people, favorites, stacks, delete, replace, trash and independent maintenance are unavailable. |

The synthetic implementation and compatibility gate is complete. Strict
aggregate evidence is committed in
[`phase10-source-import-2026-08-24.json`](../evidence/phase10-source-import-2026-08-24.json),
and paired raw measurements are committed in
[`phase10-source-import-benchmark-2026-08-24.json`](../../benchmarks/evidence/phase10-source-import-benchmark-2026-08-24.json).
Gitea [run 5675](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5675)
produced both reports on exact source SHA
`f8c13eb4d9c7cfab968b1355004ae403a146d0f9`.
The full Phase 10 gate remains open only for the authorized personal-export
shadow required by ADR-0031; no personal source has been used.
