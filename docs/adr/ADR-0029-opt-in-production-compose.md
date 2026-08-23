# ADR-0029: Opt-in production Compose override

- Status: Accepted
- Date: 2026-08-24
- Owners: project maintainers
- Supersedes: ADR-0026 server-aware container boundary only

## Context

ADR-0026 established a hardened, network-disabled Compose planner while the
client still accepted only loopback servers. ADR-0027 and ADR-0028 later
authorized verified remote HTTPS for read-only archive, immutable folder
upload and plan-bound Google Takeout import. Requiring every container user to
invent a network and secret override would make the proven binary harder to
deploy consistently and easier to weaken accidentally.

The offline planner must remain the default. Container packaging must not turn
the CLI-only production acknowledgements into persistent configuration or
embed an API key in Compose.

## Decision

The base `compose.yaml` remains non-root, read-only, capability-free and
`network_mode: none`. The repository additionally supplies
`deploy/compose.production.yaml` as an explicit second file. It changes only
the network mode to the isolated Docker bridge and mounts one operator-selected
regular API-key file through a Compose secret.

The override exposes no port, never uses host networking and does not relax the
base service's user, filesystem, PID or capability controls. It may select the
server and other non-secret limits through the existing environment/TOML
layers. `--authorize-production-read`, `--authorize-production-write`, exact
plan digest, maximum mutation count and backup reference remain command-line
only on every invocation.

This packaging adds no client capability. Endpoint validation, TLS and
hostname verification, private-CA bounds, least-privilege guidance, immutable
plans, dry-run isolation and mutation authorization remain governed by
ADR-0027 and ADR-0028. No supported image exists until the signed release gate
passes.

## Consequences

Offline planning stays safe by default. An operator must name both Compose
files, an external secret path and all production acknowledgements to reach a
remote server. Environment and TOML remain reusable because neither can enable
production access.

The built-in bridge permits outbound HTTPS without exposing the container or
sharing the host network. Deployments needing a managed network may create an
equivalent operator-owned override, but must retain the same restrictions.

## Verification

Push CI renders both the offline base and the combined production override.
The static container checker requires the secret-file mapping and bridge mode,
rejects host networking, ports, privilege, inline API keys and persisted
authorization flags, and retains all ADR-0026 base hardening checks. Remote
behavior remains covered by the synthetic Phase 7/8 private-CA disposable
gates; no production endpoint or credential enters CI.
