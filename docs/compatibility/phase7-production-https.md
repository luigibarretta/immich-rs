# Phase 7 production HTTPS compatibility matrix

Phase 7 permits the proven folder-upload and read-only archive operations over
a remote HTTPS origin. The authorization flags are CLI-only and do not broaden
the operation set: Google Takeout and Apple Photos remain read-only planners,
and no delete, replace or metadata mutation command exists.

| Surface | Declared outcome | Evidence |
|---|---|---|
| Remote transport | Supported | HTTPS only, with certificate and hostname verification; an optional bounded private-CA bundle is supported. |
| Untrusted certificate | Fail closed | The disposable gate exits with transport class 7 before authentication or mutation. |
| Authentication | Fail closed | A missing key exits with authentication class 5 without issuing a request. |
| Server compatibility | Fail closed | An incompatible version exits with compatibility class 6 before upload. |
| Read authorization | Capability-limited | `--authorize-production-read` can create archive/read access but cannot become an upload capability. |
| Write authorization | Exact binding | Both acknowledgements, plan SHA-256, expected operation count and backup reference are required. A mismatch exits with usage class 2 before mutation. |
| Dry-run | Offline | It reads no key, accepts no server transport option and reports four planned uploads without a network client. |
| 429 and 5xx | Bounded convergence | Each injected fault retries once and converges once. |
| Disconnect | Bounded convergence | The retry is capped and creates one logical result. |
| Commit with lost response | Idempotent | The retry observes the committed asset and converges without duplication. |
| Cancellation and resume | Clean convergence | Cancellation exits 130; the unchanged rerun resumes through its bounded checkpoint. |
| Fresh checkpoint | Duplicate convergence | Four previously created operations are observed as four duplicates. |
| Read-only archive | Supported | Four original streams are inventoried through the production-read capability without a mutating method. |
| Cleanup | Exact | Zero labelled containers, volumes and networks remain; ephemeral credentials and private keys are removed. |

The gate uses only a synthetic corpus and disposable Immich v3.1.0 behind a
dedicated private-CA TLS origin on a no-masquerade Docker network. The
[committed evidence](../evidence/phase7-disposable-production-2026-08-23.json)
is bound to implementation SHA
`db6ec2185b0e2bff8be8cb017f0fe8fafba95eb8` and Gitea run 5544. This evidence
does not authorize a production test or claim compatibility outside the
declared Immich v3.1.x range.
