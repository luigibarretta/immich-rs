# ADR-0026: OCI container and layered configuration

- Status: Accepted
- Date: 2026-08-22
- Owners: project maintainers

## Context

ADR-0005 defines `CLI > environment > explicit config file > defaults`, but
only CLI flags and the API-key environment variable are currently implemented.
ADR-0014 permits containers only as secondary packaging and requires a
non-root process with a read-only root filesystem. Users need a reproducible
Compose deployment without weakening the release or production boundaries.

## Decision

The CLI accepts a versioned, strict TOML file selected by global `--config` or
`IMMICH_RS_CONFIG`. Every non-secret command option can also be supplied by an
explicit `IMMICH_RS_*` environment variable. CLI values replace environment
values, which replace TOML values, which replace built-in bounded defaults.
Unknown TOML keys, unknown `IMMICH_RS_*` variables, invalid UTF-8 and invalid
values fail closed. `config show` emits the effective non-secret configuration
as stable JSON for automation and diagnosis.

API keys remain outside TOML. They are accepted either through
`IMMICH_RS_API_KEY` or a bounded regular file named by
`IMMICH_RS_API_KEY_FILE`; configuring both is an authentication failure.
Dry-run ignores inherited server configuration and still cannot construct a
network client.

The repository provides one multi-stage OCI build for `linux/amd64` and
`linux/arm64`. Builder and runtime image indexes are pinned by digest. The
runtime image contains only the CLI and runtime libraries, runs as UID/GID
65532 and has no shell. The supplied Compose service adds a read-only root,
all-capability drop, `no-new-privileges`, a private PID namespace, no network
for the default read-only planner and read-only source/config mounts.

Container images are secondary packaging. They use immutable version or
digest references and are not published from ordinary push CI. A manual
multiarch build proves both Linux variants without publishing them. A signed
release workflow may publish the exact multiarch image only after all
ADR-0025 native artifacts, signing and provenance gates pass. No `latest` tag
is a supported deployment input.

## Consequences

Compose and `docker run` users can configure every existing command without
rebuilding the image. Strict configuration catches misspellings instead of
silently using defaults, while secret files integrate with Compose secrets.
The multiarch build needs emulation on an x86-only CI host and remains separate
from fast push tests.

Container availability does not satisfy macOS, Windows, signed-tag or release
identity requirements. Server-aware commands remain restricted to a literal
loopback origin; the default Compose planner cannot contact any Immich server.

## Verification

Rust integration tests cover TOML parsing, environment precedence, CLI
precedence, strict unknown-key rejection, redacted effective output and secret
file behavior. Push CI builds and runs the amd64 image under the documented
hardening controls and validates Compose. The manual container workflow builds
both OCI platforms from the exact SHA, records metadata and leaves no image or
builder resource behind. Architecture checks enforce pinned bases, non-root
execution, read-only Compose settings, no `latest` tag and no production host
network access.
