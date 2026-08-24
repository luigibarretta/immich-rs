# Immich-to-Immich migration compatibility matrix

This vertical binds one read-only source capability and one separately
authorized destination capability into immutable `migration-plan-v1`.
Production endpoints remain outside ADR-0032; the current command accepts two
distinct disposable loopback origins only.

| Surface | Current outcome | Evidence |
|---|---|---|
| Source inventory | Verified | Timeline originals, capture time, description, location and owned-album membership are bounded and sorted. |
| Source immutability | Verified | Replanning after first apply, resume and duplicate convergence is byte-identical; the source client type has no mutation method. |
| Original streaming | Verified | Planning and apply verify SHA-1/SHA-256 while staging at most one source original; no full-library buffering exists. |
| Destination apply | Verified | Three assets/214 bytes, three metadata assignments, one album and one membership migrated on Immich v3.1.0. |
| Checkpoint resume | Verified | Eight durable effects resume without network mutation; a fresh checkpoint converges three duplicates. |
| Fault handling | Verified | Mock coverage includes auth/version refusal, timeout, 429, 5xx, disconnect and response loss after commit. |
| immich-go v0.32.0 `from-immich` | Declared parity plus extension | Originals, description/location, album and source immutability match. immich-rs additionally preserves linked Live Photo motion and contains the oracle's five job writes. |
| Real-server benchmark | Verified; scoped claim | Fresh source/destination owners per tool migrate the same 64 MiB/eight-asset corpus between the same two disposable servers at concurrency one; median wall time was 1.395 s for immich-rs and 124.048 s for immich-go, with disjoint ranges. |
| Production migration | Not authorized | ADR-0032 does not authorize remote production origins or production credentials. |
| Authorized non-production export | Pending | Aggregate-only evidence from an explicitly authorized non-production library remains external. |
| Unsupported mutations | Absent | Source writes and destination delete, replace, trash, people, tags, stacks and maintenance commands do not exist. |

The synthetic two-server and black-box compatibility gate is complete. Its
aggregate report is committed in
[`phase11-disposable-migration-2026-08-24.json`](../evidence/phase11-disposable-migration-2026-08-24.json),
with paired real-server raw measurements in
[`phase11-real-2026-08-24.json`](../../benchmarks/evidence/phase11-real-2026-08-24.json).
Gitea [run 5678](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5678)
produced and validated both reports on exact source SHA
`88d9a59d1ca1d0cd92b3ef28349ff4b95a5f8095`.
The broader Phase 11 gate remains open for the authorized non-production
export required by ADR-0032; production migration remains deliberately absent.
