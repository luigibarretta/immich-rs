# ADR-0015: Phased delivery and production cutover

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

A rewrite can look complete while missing rare metadata behavior, recovery or
operational packaging. Replacing the trusted tool before shadow evidence would
put the photo library at risk.

## Decision

Delivery follows the phases in `ROADMAP.md`. Each phase is a vertical slice
with compatibility, security, fault and performance evidence. Until a phase
gate is met, the CLI fails explicitly for that capability.

Production adoption proceeds through:

1. synthetic tests only;
2. disposable Immich integration tests;
3. read-only shadow plan against an explicitly authorized production source;
4. bounded canary import into a disposable/test owner;
5. selected real import with verified backup and rollback;
6. general use while retaining the pinned immich-go rollback tool.

No scheduled test mutates the production library. `immich-go` remains the
periodic read-only compatibility smoke and operational fallback until a later
ADR records complete replacement evidence. Version 0.1.0 cannot be published
before folder upload reaches the disposable-server gate.

## Consequences

There is a period where two tools are maintained. The production library never
becomes the first place a new behavior is exercised, and rollback remains
simple.

## Verification

Phase evidence is committed or linked from immutable CI artifacts. Cutover
checklists include exact versions, fixture/source digest, backup, owner, API
permissions, expected mutations, postconditions and rollback. A capability is
not described as supported before its gate passes.
