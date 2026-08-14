# Agent instructions

This repository is an architecture-first Rust reimplementation effort.

Before changing code:

1. Read `README.md`, `ROADMAP.md`, `CONTRIBUTING.md` and `docs/adr/README.md`.
2. Read every accepted ADR that intersects the task.
3. Treat `/home/ansible/repos/immich-go` and the pinned v0.32.0 executable as
   black-box behavioral references, not sources to port line by line.
4. Never use production Immich credentials, endpoints or personal media in a
   development test unless the user explicitly authorizes a bounded read-only
   shadow test.

Implementation rules:

- comments, identifiers and technical docs are English;
- `unsafe`, unbounded concurrency and whole-media buffering are forbidden;
- preserve crate boundaries and normalized-plan contracts;
- make the smallest vertical slice and prove it with golden, differential,
  fault-injection and benchmark evidence appropriate to the change;
- do not claim performance wins without comparable measurements;
- do not mark a phase complete while an acceptance item in `ROADMAP.md` is
  missing;
- update or supersede ADRs when a decision changes; never silently contradict
  an accepted record;
- use Git as `luigibarretta`, preserve unrelated work and push only verified
  commits.

The repository is currently a non-functional scaffold. Keep that fail-closed
behavior until Phase 1 has complete acceptance evidence.
