# CLAUDE.md — immich-rs

Canonical root: `/home/ansible/repos/immich-rs`.

Load the project memory for this exact encoded path, then follow `AGENTS.md`.
The accepted ADRs are binding architecture. Repository source and tests are
new Rust work; do not translate or copy implementation code from immich-go.

The current goal is not feature count. It is a trustworthy differential
harness and one bounded vertical slice at a time. Keep real Immich data and
secrets outside the repository. Run the complete README gate before every
commit and perform Git operations as `luigibarretta`.
