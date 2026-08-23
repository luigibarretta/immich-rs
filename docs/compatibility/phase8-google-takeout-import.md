# Phase 8 Google Takeout import compatibility matrix

Phase 8 turns the immutable read-only Takeout plan into one bounded import
capability. It preserves the Phase 3 reconciliation contract, uses only the
pinned immich-go v0.32.0 executable as a black-box oracle and adds no delete,
replace, trash or independent maintenance operation.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Directory and split ZIP | Equivalent | The complete synthetic fixture produces byte-identical `upload-plan-v2` output for a decompressed tree and two ZIP parts. |
| Source verification | Fail closed | Apply rescans the exact input set and rejects changed length, digest, path, archive entry or reconciliation configuration before mutation. |
| Archive staging | Bounded | At most one ZIP media entry and its optional XMP are streamed to plan-bound staging files; the complete export is never extracted or buffered. |
| Dry-run | Offline | The source is verified and three uploads are reported without reading a credential, creating a checkpoint or constructing a network client. |
| Mutation budget | Exact upper bound | The plan binds three asset uploads, three metadata assignments, one album create and one membership request: eight maximum mutations. |
| Production authorization | Exact binding | Server, plan SHA-256, maximum mutation count and hashed backup reference must match; a mismatch exits before checkpoint or mutation. |
| First apply | Converged | Three assets, three normalized metadata assignments, one exact album and one membership request are observed on disposable Immich v3.1.0. |
| Resume | Effect-level | `checkpoint-v2` skips all eight durable effects without repeating a committed mutation. |
| Fresh checkpoint | Duplicate convergence | The equivalent split-ZIP view observes all three assets as duplicates and converges metadata and album state. |
| Lost responses | Reconciled | Mock faults after upload, metadata update, album create and membership commit converge through observable state without blind duplicate mutation. |
| Retry and cancellation | Bounded | 429, 5xx and disconnect classes retain capped retry budgets; cancellation stops scheduling and resumes from the last durable effect. |
| Performance corpus parity | Exact | Each retained tool run observes eight visible assets, eight metadata assignments and zero retries on the same 64 MiB synthetic corpus. |
| Unsupported mutations | Absent | No delete, replace, trash, tag, people, stack or independent metadata/album maintenance command exists. |
| Cleanup | Exact | The gate leaves zero labelled containers, volumes, networks, staging files, credentials or private keys. |

The public functional fixture includes the Phase 3 supplemental-filename
divergence: immich-rs deliberately resolves that documented Google sidecar
form while the oracle leaves the asset pending. The performance corpus uses
only the conventional sidecar form so the two tools reach identical observable
postconditions before timings are compared.

The disposable HTTPS gate is green in Gitea
[run 5570](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5570)
on implementation SHA `5172c73fe615658d3a90d5d5849ca2209142db12`.
Its [committed aggregate evidence](../evidence/phase8-disposable-takeout-2026-08-23.json)
is enforced by push CI. The paired benchmark is green in
[run 5578](https://git.luigibarretta.com/luigibarretta/immich-rs/actions/runs/5578)
on SHA `4edae0365ac5137a2ee99d216755d8e3693cf9c8`; the
[raw report](../../benchmarks/evidence/phase8-2026-08-24.json) binds both
binaries, fixture, environment, six retained pairs and all aggregate claims.

This matrix declares compatibility only for Google Takeout import against the
Immich v3.1.x API range. Apple Photos apply and all destructive mutations remain
outside the accepted capability boundary.
