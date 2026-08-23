# Migration and rollback from immich-go

No supported immich-rs release is published yet. A source-built client from an
exact green revision may use only the Phase 7/8 production surface: verified
remote HTTPS, read-only archive, immutable folder upload and plan-bound Google
Takeout import. Every remote read/write acknowledgement is CLI-only; no
configuration file, environment variable or Compose file can silently enable
production. Delete, replace, trash and independent maintenance remain absent.

## Compare without mutation

1. Keep the verified immich-go v0.32.0 binary and its pinned SHA-256 from
   `tests/oracle/baseline.toml` available as the rollback tool.
2. Run immich-rs folder, Google Takeout or Apple Photos planning against a copy
   or read-only source. Save the normalized plan, source identity, tool version
   and warnings/errors.
3. Run the documented black-box comparison only with synthetic fixtures and a
   loopback mock. Do not point the oracle dry-run at production: its five known
   job-resume PUTs are contained only by the mock.
4. Resolve every declared divergence and fail-closed diagnostic before a
   disposable import is considered.

## Disposable canary

Create a new disposable Immich stack and owner with a scoped API key. Run
`plan upload folder`, `apply upload --dry-run`, first apply and idempotent
resume. Verify asset counts, Live Photo links, source/server checksums and zero
unexpected requests. Remove the exact key, owner, containers, volumes, network,
workspace and downloaded images after evidence capture.

For Google Takeout, additionally prove directory/split-ZIP plan equality,
normalized metadata, exact album membership, effect-level resume and
fresh-checkpoint duplicate convergence. Never use personal media for this
canary; the repository gate provides a complete synthetic fixture.

For a read-only archive canary, run `plan archive immich`, first apply and
verified resume against the same disposable instance. Compare the original-byte
digest multiset before cleanup.

## Authorized production cutover

1. Verify an Immich backup or restore point independently and retain its
   bounded operator reference.
2. Create a temporary least-privilege API key for only the selected plan. A
   Takeout import may require asset upload/read/update and album
   read/create/membership permissions; it never requires delete or trash.
3. Create and inspect the immutable server-bound plan, preserve its SHA-256 and
   maximum mutation count, then run `apply upload --dry-run` without a key.
4. Apply only that plan with `--authorize-production-read`,
   `--authorize-production-write`, `--confirm-plan-sha256`,
   `--expected-operations` and `--backup-reference`. Any mismatch must fail
   before mutation.
5. Verify counts, metadata and albums against the reviewed plan, retain the
   checkpoint until acceptance, then revoke the temporary key.

Do not run immich-rs and immich-go mutations over the same source selection.
The periodic homelab immich-go smoke remains unchanged. See the
[Phase 7 transport matrix](compatibility/phase7-production-https.md) and
[Phase 8 Takeout matrix](compatibility/phase8-google-takeout-import.md) before
cutover.

## Rollback

Planning is read-only and needs no data rollback: discard the plan and return
to the pinned immich-go workflow. For a disposable canary, delete only the
labelled disposable stack after exporting its evidence. Never use immich-rs
delete/replace commands—none exist.

If an authorized production import is interrupted, stop new writes, retain the
immutable plan/checkpoint and verify server state before retrying.
Do not rerun both tools against the same selection. Restore the verified Immich
backup if postconditions fail, revoke the candidate API key and resume the
pinned immich-go operational workflow only after the library is consistent.
