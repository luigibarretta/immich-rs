# ADR-0020: Containerized disposable transport

- Status: Accepted
- Date: 2026-08-15
- Owners: project maintainers
- Supersedes: ADR-0019

## Context

ADR-0019 requires the upload CLI to see only a loopback Immich origin and puts
the disposable stack on a dedicated Docker bridge without IP masquerading. Its
host execution topology publishes Immich to an ephemeral host `127.0.0.1`
port. A containerized Gitea job has a different network namespace: its
loopback cannot reach a port bound to the Docker host loopback. The first
manual workflow therefore timed out without reaching Immich, while its cleanup
still removed every labelled resource.

Using host networking or the Docker host gateway would make unrelated host
listeners reachable and would weaken the production-isolation gate. Accepting
the disposable container address in the CLI would weaken the loopback-only
capability boundary.

## Decision

This ADR incorporates every Phase 2 decision in ADR-0019 unchanged except the
disposable HTTP transport described below.

Host execution continues to publish only Immich HTTP to an ephemeral
`127.0.0.1` port. Containerized execution verifies its current container ID
through the Docker API, attaches that one driver container to the labelled
non-masquerading disposable bridge, and does not publish an Immich host port.
A repository-owned proxy accepts one connection at a time on the driver
container's `127.0.0.1` and forwards bounded chunks only to the disposable
server name and port on that bridge.

The CLI continues to receive only a loopback URL. Host networking, host-gateway
access, non-loopback CLI exceptions and general-purpose proxy destinations are
forbidden. The proxy has one fixed target, at most one active connection, a
64-KiB transfer buffer and cooperative signal handling. It stores no body and
does not log credentials or payloads.

Cleanup stops the exact proxy process and disconnects the verified driver
container before removing the labelled stack network. Failure at any stage
must still leave zero labelled containers, volumes and networks. The driver is
never a disposable stack dependency: database, Valkey and Immich remain on
only the isolated bridge.

## Consequences

The effective security boundary is identical in both modes: the tested process
can address Immich only through its own loopback, and the disposable service
containers have no masqueraded egress. Containerized CI gains one temporary
secondary network attachment, but no host network or production endpoint.

The small proxy becomes test infrastructure that must obey repository LOC,
bounded-I/O and provenance checks. Docker cleanup must account for a network
endpoint that is not itself a labelled disposable container.

## Verification

Tooling tests exercise bidirectional proxy transfer and orderly termination.
The manual Gitea workflow must prove the containerized topology on its exact
SHA, preserve aggregate evidence and complete with zero labelled resources.
Host execution remains covered by the committed real-server evidence from
ADR-0019. Architecture checks reject host networking and host-gateway use in
the disposable workflow and harness.
