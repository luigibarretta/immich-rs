# Container and Compose deployment

No supported container release has been published yet. The repository can
build a local image from the exact checked-out source, and the release workflow
can produce an immutable multiarch OCI archive once every Phase 6 prerequisite
is available. Do not treat `immich-rs:local` as a published release.

## Offline folder planning with Compose

The supplied service runs the read-only folder planner with no network:

```bash
cp deploy/immich-rs.env.example deploy/immich-rs.env
cp deploy/immich-rs.example.toml deploy/immich-rs.toml
IMMICH_RS_SOURCE_PATH=/absolute/path/to/synthetic-or-authorized-media \
IMMICH_RS_ENV_FILE=./deploy/immich-rs.env \
IMMICH_RS_CONFIG_PATH=./deploy/immich-rs.toml \
  docker compose build --pull
IMMICH_RS_SOURCE_PATH=/absolute/path/to/synthetic-or-authorized-media \
IMMICH_RS_ENV_FILE=./deploy/immich-rs.env \
IMMICH_RS_CONFIG_PATH=./deploy/immich-rs.toml \
  docker compose run --rm immich-rs > normalized-plan.json
docker compose down --volumes --remove-orphans
```

The source bind is mounted at `/source` in read-only mode. The named `/state`
and `/output` volumes are available for commands that need durable local
state. The example itself reads only the small synthetic file under
`deploy/example-source` and is safe to run unchanged.

Set `IMMICH_RS_GID` to a group that can read the source tree when its host
permissions do not allow GID 65532 to read it. Do not run the container as
root to bypass permissions. Set `IMMICH_RS_IMAGE` to an immutable image digest
when consuming a future supported release.

The service is deliberately hardened with UID 65532, no capabilities,
`no-new-privileges`, a read-only root, bounded PIDs, a small temporary
filesystem and `network_mode: none`. It exposes no port and has no shell.

## Configuration

The example uses both supported configuration layers:

- `IMMICH_RS_CONFIG` in the environment selects the mounted strict TOML file;
- every command option can be set with its documented `IMMICH_RS_*` variable;
- an explicit command-line value has final precedence.

The deterministic order is `CLI > environment > TOML > bounded default`.
Unknown project variables and unknown TOML keys fail closed. See the complete
[configuration matrix](configuration.md), including limits and path options.

API keys are never valid TOML values. For a server-aware invocation, mount a
secret as a regular file and select it with `IMMICH_RS_API_KEY_FILE`. Never put
an API key in the Compose file, image, source tree or committed environment
file. Configuring both key variables is an error.

## Server-aware container boundary

Upload and archive commands accept only literal loopback Immich origins. The
default Compose service has no network and therefore cannot contact Immich.
This is intentional: container packaging does not authorize production use or
broaden the Phase 2/5 safety boundary.

Testing a server-aware command requires an explicitly disposable Immich whose
network namespace is shared with the test container, disposable credentials
and complete resource cleanup. The repository integration harnesses create
that topology themselves. Do not change `network_mode` to a production network
or weaken the loopback validation.

## Verified multiarch OCI archive

Run the separate build only from a clean exact checkout and on a host
authorized to use privileged binfmt:

```bash
mkdir -p .artifacts/container
scripts/build-multiarch-container.sh \
  --output .artifacts/container/immich-rs.oci.tar \
  --report .artifacts/container/report.json \
  --revision "$(git rev-parse HEAD)" \
  --version 0.0.0
```

The script builds `linux/amd64` and `linux/arm64` from digest-pinned builder,
runtime, BuildKit, binfmt and SBOM-generator images. It emits one OCI archive
with per-platform SPDX SBOM and SLSA provenance attestations, then verifies
platforms, UID/GID, labels and archive digest. It creates a uniquely named
Buildx builder and removes it on exit. If it installed `qemu-aarch64`, it also
removes only that registration.

The manual Gitea workflow retains only the bounded JSON verification report
and deletes its unsigned OCI archive. An RC workflow may retain the archive,
but it enters the final checksum/signature bundle only after all five native
targets and the maintainer-controlled OpenPGP identity satisfy ADR-0025.

Verify the source contract without a multiarch build:

```bash
python3 scripts/check-container.py
docker compose config --quiet
bash -n scripts/build-multiarch-container.sh
```
