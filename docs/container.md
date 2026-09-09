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

The default Compose service has no network and therefore cannot contact
Immich. Keep it as the safe planner default. The binary also supports the
Phase 7/8 remote HTTPS surface through the supplied
`deploy/compose.production.yaml` override with outbound networking and a
secret file. Its complete contract is:

```yaml
services:
  immich-rs:
    network_mode: bridge
    environment:
      IMMICH_RS_API_KEY_FILE: /run/secrets/immich_rs_api_key
    secrets:
      - immich_rs_api_key

secrets:
  immich_rs_api_key:
    file: ${IMMICH_RS_API_KEY_PATH:?set an absolute secret-file path}
```

Use both files and pass every production authorization as an explicit command
argument:

```bash
IMMICH_RS_API_KEY_PATH=/absolute/path/to/api-key \
  docker compose -f compose.yaml -f deploy/compose.production.yaml \
  run --rm immich-rs \
  apply upload --server https://immich.example.invalid \
  --plan /state/upload-plan.json --source /source \
  --checkpoint /state/checkpoint.sqlite \
  --authorize-production-read --authorize-production-write \
  --confirm-plan-sha256 <64-hex-digest> --expected-operations <count> \
  --backup-reference <verified-restore-point-reference>
```

The override must not use host networking, expose ports, embed the API key or
remove the base service's non-root/read-only/capability controls. Production
acknowledgements intentionally cannot be stored in environment or TOML. Test
transport changes first against an explicitly disposable Immich; the
repository integration harnesses create and remove that isolated topology.

## Optional Web Console service

The `web` profile builds a separate `Containerfile.web` image and does not
change the default CLI service. It contains only `immich-rs-web`, its runtime
libraries and license notices. It contains no operator configuration, API key,
OIDC client secret, TLS key, certificate or CA bundle. The default configuration
bind is `/dev/null` and the repository's example secret directory is explicitly
empty, so starting the service without operator input fails closed.

The profile publishes only `127.0.0.1:2285` by default. Container networking
requires the process to listen on `0.0.0.0:2285`, so this deployment uses the
complete direct-TLS/OIDC LAN policy even though Docker publishes its host port
only on loopback. Follow the [LAN configuration contract](web-console-lan.md)
and use container paths in the strict TOML:

- `/sources` for the read-only configured source root;
- `/run/secrets` for read-only API keys, TLS identity, OIDC secret and CA files;
- `/state` for the private bounded history, plans and executor checkpoints;
- `/staging` as a separate writable reservation. The current import executor
  creates its short-lived staging beside the private checkpoint under
  `/state/checkpoints` and removes it, so application-written bytes in the
  reserved mount remain zero.

Prepare the source/config/secret paths with permissions readable by UID 65532,
then build and start only the optional profile:

```bash
IMMICH_RS_WEB_CONFIG_PATH=/absolute/path/to/immich-rs-web.toml \
IMMICH_RS_WEB_SOURCE_PATH=/absolute/path/to/authorized-source \
IMMICH_RS_WEB_SECRETS_PATH=/absolute/path/to/private-secret-directory \
  docker compose --profile web build --pull immich-rs-web
IMMICH_RS_WEB_CONFIG_PATH=/absolute/path/to/immich-rs-web.toml \
IMMICH_RS_WEB_SOURCE_PATH=/absolute/path/to/authorized-source \
IMMICH_RS_WEB_SECRETS_PATH=/absolute/path/to/private-secret-directory \
  docker compose --profile web up immich-rs-web
docker compose --profile web down --volumes --remove-orphans
```

The root filesystem is read-only, the process is UID/GID 65532 with all
capabilities dropped and `no-new-privileges`, and only the named state and
staging volumes are writable. The source, configuration and secret directory
are read-only binds. The PID limit, temporary filesystem and shutdown grace
period are explicit. Set `IMMICH_RS_WEB_IMAGE` to an immutable version or digest
for a published candidate; `latest` is unsupported. Rootless engines still
require the operator bind paths and published port to be accessible through
their user namespace. Back up and restore the complete state volume only while
the service is stopped; see the
[Web Console operator and recovery guide](web-console-operations.md).

## Verified multiarch OCI archive

Run the separate build only from a clean exact checkout and on a host
authorized to use privileged binfmt:

```bash
mkdir -p .artifacts/container
scripts/build-multiarch-container.sh \
  --product immich-rs \
  --output .artifacts/container/immich-rs-0.0.0-linux-multiarch.oci.tar \
  --report .artifacts/container/immich-rs-0.0.0-linux-multiarch.container.json \
  --revision "$(git rev-parse HEAD)" \
  --version 0.0.0
scripts/build-multiarch-container.sh \
  --product immich-rs-web \
  --output .artifacts/container/immich-rs-web-0.0.0-linux-multiarch.oci.tar \
  --report .artifacts/container/immich-rs-web-0.0.0-linux-multiarch.container.json \
  --revision "$(git rev-parse HEAD)" \
  --version 0.0.0
```

The script builds `linux/amd64` and `linux/arm64` from digest-pinned builder,
runtime, BuildKit, binfmt and SBOM-generator images. Each explicit product emits
its own OCI archive with per-platform SPDX SBOM and SLSA provenance
attestations, then verifies the exact product label, platforms, UID/GID,
revision and archive digest. Each invocation creates a uniquely named Buildx
builder and removes it on exit. If it installed `qemu-aarch64`, it also removes
only that registration.

The manual Gitea workflow retains only both bounded JSON verification reports
and deletes both unsigned OCI archives. An RC workflow may retain the archives,
but they enter one final checksum/signature bundle only after both binaries on
all five native targets and the maintainer-controlled OpenPGP identity satisfy
ADR-0025 and ADR-0033.

Verify the source contract without a multiarch build:

```bash
python3 scripts/check-container.py
docker compose config --quiet
bash -n scripts/build-multiarch-container.sh
```
