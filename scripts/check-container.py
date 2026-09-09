#!/usr/bin/env python3
"""Enforce the secondary OCI packaging and hardened Compose contract."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
PINNED_IMAGE = re.compile(r"^ARG [A-Z_]+=[^\s@]+@sha256:[0-9a-f]{64}$", re.MULTILINE)


def check(root: Path = REPOSITORY_ROOT) -> list[str]:
    """Return every container contract violation under one repository root."""
    failures: list[str] = []
    containerfile = read(root / "Containerfile", failures)
    web_containerfile = read(root / "Containerfile.web", failures)
    compose = read(root / "compose.yaml", failures)
    production_compose = read(root / "deploy/compose.production.yaml", failures)
    dockerignore = read(root / ".dockerignore", failures)
    environment = read(root / "deploy/immich-rs.env.example", failures)
    provenance = read(root / "deploy/example-source/README.md", failures)
    empty_web_secrets = read(root / "deploy/example-web-secrets/README.md", failures)
    multiarch = read(root / "scripts/build-multiarch-container.sh", failures)
    web_test = read(root / "scripts/test-web-container.sh", failures)
    config_path = root / "deploy/immich-rs.example.toml"

    pins = PINNED_IMAGE.findall(containerfile)
    required_pins = ("RUST_IMAGE=", "RUNTIME_IMAGE=")
    if len(pins) != 2 or any(not any(name in pin for pin in pins) for name in required_pins):
        failures.append("Containerfile must pin builder and runtime image digests")
    required_container = (
        "cargo build --locked --release -p immich-rs-cli",
        "USER 65532:65532",
        'ENTRYPOINT ["/usr/local/bin/immich-rs"]',
        "COPY --from=builder --chown=65532:65532",
        "COPY --chown=65532:65532 LICENSE NOTICE.md /licenses/immich-rs/",
    )
    failures.extend(
        f"Containerfile is missing: {token}" for token in required_container if token not in containerfile
    )
    forbidden_container = ("apt-get", "curl ", "wget ", ":latest", "--privileged")
    failures.extend(
        f"Containerfile contains forbidden token: {token}"
        for token in forbidden_container
        if token in containerfile
    )
    web_pins = PINNED_IMAGE.findall(web_containerfile)
    if len(web_pins) != 2 or any(
        not any(name in pin for pin in web_pins) for name in required_pins
    ):
        failures.append("Containerfile.web must pin builder and runtime image digests")
    required_web_container = (
        "cargo build --locked --release -p immich-rs-web --bin immich-rs-web",
        "USER 65532:65532",
        'ENTRYPOINT ["/usr/local/bin/immich-rs-web"]',
        "COPY --from=builder --chown=65532:65532",
        "COPY --chown=65532:65532 LICENSE NOTICE.md /licenses/immich-rs-web/",
        'org.opencontainers.image.title="immich-rs-web"',
        "install -d -m 0700 /out/staging /out/state",
    )
    failures.extend(
        f"Containerfile.web is missing: {token}"
        for token in required_web_container
        if token not in web_containerfile
    )
    forbidden_web_container = forbidden_container + (
        "COPY deploy", "COPY . /", "IMMICH_RS_WEB_CONFIG=",
    )
    failures.extend(
        f"Containerfile.web contains forbidden token: {token}"
        for token in forbidden_web_container
        if token in web_containerfile
    )

    required_compose = (
        "network_mode: none",
        "read_only: true",
        'user: "65532:${IMMICH_RS_GID:-65532}"',
        'cap_drop: ["ALL"]',
        "no-new-privileges:true",
        "pids_limit: 128",
        "read_only: true",
        "IMMICH_RS_CONFIG_PATH",
        "IMMICH_RS_SOURCE_PATH",
        "immich-rs:local",
    )
    failures.extend(
        f"Compose is missing: {token}" for token in required_compose if token not in compose
    )
    cli_compose = compose.split("\n  immich-rs-web:", maxsplit=1)[0]
    forbidden_compose = (
        "network_mode: host",
        "host-gateway",
        "host.docker.internal",
        "privileged: true",
        ":latest",
        "IMMICH_RS_API_KEY:",
    )
    failures.extend(
        f"Compose contains forbidden token: {token}" for token in forbidden_compose if token in compose
    )
    if "ports:" in cli_compose:
        failures.append("offline CLI Compose service must not publish ports")
    required_web_compose = (
        'profiles: ["web"]',
        "immich-rs-web:0.0.0",
        "dockerfile: Containerfile.web",
        "IMMICH_RS_WEB_CONFIG: /config/immich-rs-web.toml",
        "IMMICH_RS_WEB_CONFIG_PATH",
        "IMMICH_RS_WEB_SOURCE_PATH",
        "IMMICH_RS_WEB_SECRETS_PATH",
        "target: /sources\n        read_only: true",
        "target: /run/secrets\n        read_only: true",
        '"127.0.0.1:2285:2285"',
        'user: "65532:65532"',
        "source: immich_rs_web_state",
        "source: immich_rs_web_staging",
        "stop_grace_period: 15s",
    )
    failures.extend(
        f"Web Compose service is missing: {token}"
        for token in required_web_compose
        if token not in compose
    )
    forbidden_web_compose = (
        "0.0.0.0:2285:2285", "network_mode: host", "privileged: true",
        "IMMICH_RS_WEB_API_KEY", "IMMICH_RS_WEB_CLIENT_SECRET", "IMMICH_RS_WEB_TLS",
    )
    web_compose = compose.removeprefix(cli_compose)
    failures.extend(
        f"Web Compose service contains forbidden token: {token}"
        for token in forbidden_web_compose
        if token in web_compose
    )
    required_production_compose = (
        "network_mode: bridge",
        "IMMICH_RS_API_KEY_FILE: /run/secrets/immich_rs_api_key",
        "file: ${IMMICH_RS_API_KEY_PATH:?set an absolute API-key file path}",
    )
    failures.extend(
        f"Production Compose override is missing: {token}"
        for token in required_production_compose
        if token not in production_compose
    )
    forbidden_production_compose = (
        "network_mode: host", "ports:", "privileged: true",
        "IMMICH_RS_API_KEY:", "authorize-production",
    )
    failures.extend(
        f"Production Compose override contains forbidden token: {token}"
        for token in forbidden_production_compose
        if token in production_compose
    )
    required_ignore = (
        "**",
        "!Cargo.lock",
        "!Cargo.toml",
        "!Containerfile.web",
        "!LICENSE",
        "!NOTICE.md",
        "!crates/**",
    )
    failures.extend(
        f".dockerignore is missing: {token}" for token in required_ignore if token not in dockerignore
    )
    gitignore = read(root / ".gitignore", failures)
    for local_path in (
        "/deploy/immich-rs.env",
        "/deploy/immich-rs.toml",
        "/deploy/immich-rs-web.toml",
        "/deploy/web-secrets/",
        "/normalized-plan.json",
    ):
        if local_path not in gitignore:
            failures.append(f".gitignore exposes local container data: {local_path}")
    if environment.strip() != "IMMICH_RS_CONFIG=/config/immich-rs.toml":
        failures.append("example container environment must contain only the config selector")
    if "provenance:" not in provenance or "license: CC0-1.0" not in provenance:
        failures.append("container example source lacks synthetic provenance")
    if "contains no credentials or key material" not in empty_web_secrets:
        failures.append("web secret-mount default is not explicitly empty")
    required_multiarch = (
        "--platform linux/amd64,linux/arm64",
        "--provenance=mode=max",
        '--attest "type=sbom,generator=$SBOM_SCANNER"',
        "--uninstall qemu-aarch64",
        "docker buildx rm",
        'SOURCE_REVISION=$(git -C "$ROOT" rev-parse HEAD)',
        'status --porcelain=v1 --untracked-files=all',
        "BINFMT_IMAGE=\"docker.io/tonistiigi/binfmt@sha256:",
        "SBOM_SCANNER=\"docker.io/docker/buildkit-syft-scanner@sha256:",
        "BUILDKIT_IMAGE=\"docker.io/moby/buildkit@sha256:",
    )
    failures.extend(
        f"multiarch builder is missing: {token}"
        for token in required_multiarch
        if token not in multiarch
    )
    if ":latest" in multiarch or "moby/buildkit:buildx-stable-1" in multiarch:
        failures.append("multiarch builder contains an unpinned tool image")
    required_web_test = (
        "Containerfile.web", "--network none", "--read-only", "--cap-drop ALL",
        "--security-opt no-new-privileges", "--pids-limit 128", "docker image rm",
        "trap cleanup EXIT", "cmp",
    )
    failures.extend(
        f"web container test is missing: {token}"
        for token in required_web_test
        if token not in web_test
    )
    try:
        config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        failures.append(f"cannot parse example configuration: {error}")
    else:
        if config.get("schema_version") != 1 or config.get("scan", {}).get("source") != "/source":
            failures.append("example configuration is not schema-v1 container input")
    return failures


def read(path: Path, failures: list[str]) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        failures.append(f"cannot read {path.name}: {error}")
        return ""


def main() -> int:
    failures = check()
    if failures:
        print("container contract failed:\n" + "\n".join(failures), file=sys.stderr)
        return 1
    print("container contract passed: hardened offline default and secret-backed HTTPS override")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
