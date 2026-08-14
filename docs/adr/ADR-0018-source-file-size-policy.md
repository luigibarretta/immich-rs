# ADR-0018: Source file size policy

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The project needs a mechanical signal when a source module accumulates too many
responsibilities. The sibling Rust repositories `quadro-rust-resume`, `klaxond`
and `blackstart` use a 400-line hard gate. The larger mixed-stack StudioFlow
repository uses 500 lines with historical exceptions. immich-rs is greenfield,
so the stricter sibling-Rust limit applies without exceptions.

## Decision

Maintained Rust, Python and shell source under `crates`, `scripts` and `tests`
is limited to 400 physical lines per file. The initial baseline permits zero
growth exceptions and zero ignored source paths. Any future exception requires
a narrow documented reason and may not grow beyond its recorded baseline.

The limit is a design guard, not a quality proxy. Modules are split by cohesive
responsibility and stable interfaces, never by arbitrary line ranges. Normal
senior-engineering requirements still apply: idempotent behavior, explicit
errors, bounded resources, focused tests and reviewable names.

## Consequences

Scanner discovery, reconciliation and tests remain separate modules. Large
generated or vendored assets are outside the maintained-source roots; adding
an ignore requires an explicit baseline entry. Small files can still be poorly
designed and remain subject to review.

## Verification

`scripts/check-loc.py` validates its versioned baseline, rejects stale
allowances or ignores, and fails when any maintained file exceeds 400 lines.
Tooling tests exercise the boundary and CI runs the guard on every push.
