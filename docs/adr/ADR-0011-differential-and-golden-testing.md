# ADR-0011: Differential and golden testing

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

The hardest requirement is behavioral compatibility, not compilation. The
reference behavior contains edge cases that cannot be reconstructed reliably
from documentation alone, while production photos and credentials cannot be
used as ordinary fixtures.

## Decision

Use three complementary test layers:

1. pure unit/property tests for parsers, normalization and invariants;
2. synthetic golden corpora whose expected normalized plans are versioned;
3. black-box differential runs against the exact immich-go v0.32.0 executable
   and disposable Immich test servers.

The oracle runner records command, tool version, fixture digest, normalized
result and server-side observable outcome. Volatile timestamps, IDs, ordering
without semantic meaning and presentation text are normalized before compare.
Differences are classified as regression, intentional divergence or oracle
defect; an intentional divergence requires documentation and a test.

No oracle source code or binary is vendored. CI may fetch it from the official
release with a pinned checksum, or a developer may supply the verified binary.
Tests never target production by default.

## Consequences

The harness costs substantial work before features appear, but it makes the
rewrite measurable and allows deliberate improvements over legacy behavior.

## Verification

CI verifies fixture provenance/digests and normalized goldens. A compatibility
feature cannot be marked supported without a differential case or a documented
reason the oracle has no equivalent. Negative tests prove production hostnames,
tokens and personal metadata are absent from fixtures and snapshots.
