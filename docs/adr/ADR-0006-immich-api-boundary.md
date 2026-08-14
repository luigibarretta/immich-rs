# ADR-0006: Immich API boundary and version policy

- Status: Accepted
- Date: 2026-08-14
- Owners: project maintainers

## Context

Immich evolves quickly and its OpenAPI surface can change across releases.
Spreading generated models and raw endpoints through the application would
turn every server change into a cross-project migration.

## Decision

All server communication lives in `immich-client` behind task-oriented traits
and project-owned domain types. OpenAPI may generate a private transport layer,
but generated types never cross the crate boundary and generated output is
reproducible from a pinned specification plus generator version.

At startup the client performs an authenticated about/version probe and checks
a declared compatibility range. Unknown major versions fail closed. Known
newer minor versions may run only when the invoked capability's contract tests
cover them or the user explicitly accepts an experimental override.

HTTP uses rustls, connection pooling, per-operation deadlines and structured
error bodies. Retry eligibility is returned as typed information rather than
embedded sleep loops. API keys are passed through a redacting secret type.

## Consequences

API churn is localized and mocking is straightforward. Mapping generated
models into domain types adds code, but prevents generated schema details from
becoming permanent architecture.

## Verification

Contract tests run against pinned OpenAPI fixtures and disposable supported
Immich versions. CI proves generated output is clean when generation is
introduced. Tests cover unsupported-version refusal, response redaction and
typed retry classification.
