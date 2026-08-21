# Migration and rollback from immich-go

immich-rs is not yet authorized for production use. The current upload and
archive server commands deliberately accept loopback/localhost Immich
instances only. This guide defines the future cutover sequence and the safe
rollback available today; it does not weaken that boundary.

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

For a read-only archive canary, run `plan archive immich`, first apply and
verified resume against the same disposable instance. Compare the original-byte
digest multiset before cleanup.

## Future production cutover

A production import remains blocked until a superseding ADR explicitly grants
that capability. That ADR must require a verified Immich backup, a bounded
selected source, least-privilege owner/key, immutable plan, dry-run review,
expected mutation count, cancellation checkpoint and postcondition checklist.
The periodic homelab immich-go smoke remains unchanged.

## Rollback

Planning is read-only and needs no data rollback: discard the plan and return
to the pinned immich-go workflow. For a disposable canary, delete only the
labelled disposable stack after exporting its evidence. Never use immich-rs
delete/replace commands—none exist.

If a future authorized production import is interrupted, stop new writes,
retain the immutable plan/checkpoint and verify server state before retrying.
Do not rerun both tools against the same selection. Restore the verified Immich
backup if postconditions fail, revoke the candidate API key and resume the
pinned immich-go operational workflow only after the library is consistent.
