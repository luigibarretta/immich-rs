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
| Apply and resume | Implemented, evidence pending | The plan selects the Apple adapter and uses the existing capped retry, one-entry staging and `checkpoint-v2` effect engine. |
| Production authorization | Implemented, evidence pending | Exact plan digest, maximum mutation budget, server identity and hashed backup reference are required. |
| Disposable Immich postconditions | Pending | A synthetic apply/resume/fresh-checkpoint duplicate run and zero-resource cleanup artifact are still required. |
| immich-go v0.32.0 `from-icloud` parity | Pending | Only black-box execution on the same synthetic corpus may close this row. |
| Comparable benchmark | Pending | No Apple performance claim is authorized until both tools run the same corpus, server and environment. |
| Authorized personal export | Pending | Aggregate-only shadow evidence will be produced after the maintainer supplies an explicit export. |
| Unsupported mutations | Absent | No delete, replace, trash, tag, people, stack or independent maintenance command exists. |

The implementation is covered by deterministic synthetic directory/ZIP,
configuration-drift, source-drift, cancellation, CLI authorization and bounded
staging tests. These tests establish the implementation boundary, not the
pending external compatibility and scale gates.
