# Contributing

## Before changing code

1. Read `AGENTS.md`, `CLAUDE.md`, `ROADMAP.md` and every accepted ADR relevant
   to the change.
2. State the compatibility surface being implemented.
3. Add or update synthetic golden fixtures before implementation.
4. Capture normalized black-box oracle output when an equivalent immich-go
   behavior exists.
5. Keep production URLs, credentials and personal media out of tests, logs and
   commits.

## Design rules

- All code, comments, commit messages and technical documentation are English.
- No copied or mechanically translated immich-go implementation code.
- No unbounded task spawning, queues, archive extraction or whole-file media
  buffering.
- No `unsafe`, `unwrap` or `expect` in production or test code unless a new ADR
  accepts a narrowly documented exception.
- Maintained Rust, Python and shell files stay within the 400-line source limit
  with no undocumented baseline exceptions.
- Read-only operations are idempotent: unchanged input and configuration must
  produce byte-identical normalized output. Future mutations must converge
  through stable operation IDs and durable checkpoints.
- Library crates do not print, read process environment or terminate the
  process. The CLI owns presentation and exit codes.
- API models stay behind `immich-client`; source-format details stay behind
  `immich-sources`.
- A behavior change requires a test; an architectural reversal requires a
  superseding ADR.

## Required checks

Run the commands in the README. Commits use Conventional Commits and Git
operations on the homelab are performed as `luigibarretta`.

## Fixtures

Only synthetic, generated or explicitly redistributable fixtures may be
committed. Each fixture directory must describe its provenance and expected
normalized plan. Real EXIF coordinates, names, faces, API responses, tokens or
user identifiers are forbidden.
