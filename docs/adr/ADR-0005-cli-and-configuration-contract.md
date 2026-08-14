# ADR-0005: CLI and configuration contract

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Users need predictable automation and a migration path from immich-go, but
copying every historical flag would freeze accidental behavior before the new
domain model exists. Secrets also must not become ordinary persisted config.

## Decision

The CLI uses stable subcommands and maps inputs into a validated core request.
It preserves high-value immich-go concepts and offers a documented translation
matrix, but does not promise flag-for-flag compatibility in early phases.

Configuration precedence is `CLI > environment > explicit config file >
defaults`. The effective non-secret configuration can be rendered in redacted
form. API keys use a dedicated secret environment variable, protected file or
future OS secret provider; they are never written back to config.

Human output goes to stderr, requested machine output to stdout. Stable exit
classes distinguish usage, compatibility, source, authentication, transport,
partial execution and invariant failures. Dry-run must be enforced in the core
execution capability, not only by omitting a CLI branch.

## Consequences

Migration needs a compatibility guide. Scripts gain stable JSON/event output
and reliable exits. Secret handling is slightly less convenient than one
all-inclusive config file.

## Verification

Snapshot tests cover help, redacted effective config, JSON schemas and exit
classes. Tests prove dry-run cannot construct or invoke a mutating client
capability.
