#!/usr/bin/env python3
"""Validate the fail-closed native release-candidate pipeline contract."""

from __future__ import annotations

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": "ubuntu-24.04",
    "aarch64-unknown-linux-gnu": "ubuntu-24.04-arm",
    "x86_64-apple-darwin": "macos-15-intel",
    "aarch64-apple-darwin": "macos-15",
    "x86_64-pc-windows-msvc": "windows-2025",
}


class ReleaseCheckError(ValueError):
    """The release workflow no longer satisfies ADR-0014/ADR-0025."""


def read(relative: str) -> str:
    try:
        return (ROOT / relative).read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ReleaseCheckError(f"cannot read {relative}") from error


def validate() -> None:
    workflow = read(".github/workflows/release.yml")
    workflow_lines = [line.strip() for line in workflow.splitlines()]
    for target, runner in TARGETS.items():
        if (
            workflow_lines.count(f"target: {target}") != 1
            or workflow_lines.count(f"- runner: {runner}") != 1
        ):
            raise ReleaseCheckError(f"native target mapping drifted: {target}")
    required = (
        'tags: ["v*-rc.*"]',
        "cargo install cargo-cyclonedx --version 0.5.9 --locked",
        "--spec-version 1.5",
        "RELEASE_SIGNING_PRIVATE_KEY",
        "RELEASE_SIGNING_FINGERPRINT",
        "RELEASE_SIGNING_PASSPHRASE",
        "--pinentry-mode loopback --passphrase-fd 0",
        "gpg --batch --verify",
        "merge-multiple: true",
        "needs: [native, container]",
        "build-multiarch-container.sh",
        "linux-multiarch.oci.tar",
        "immich-rs-web-${version}-linux-multiarch.oci.tar",
        "cancel-in-progress: false",
        "environment: immich-rs-release",
        "python scripts/package-release.py",
        "--product immich-rs-web",
        "cargo build --locked --release -p immich-rs-web --bin immich-rs-web",
        "crates/immich-web/immich-rs-web_bin.cdx.json",
        "web_binary: target/release/immich-rs-web.exe",
        'python scripts/check-pe.py "${{ matrix.binary }}"',
        "gh release create",
        "check-production-evidence.py",
        "check-phase8-evidence.py",
        "check-phase8-benchmark.py",
        "check-source-import-evidence.py",
        "check-migration-evidence.py",
    )
    if any(value not in workflow for value in required):
        raise ReleaseCheckError("release identity, SBOM or signing gate drifted")
    forbidden = ("workflow_dispatch", "pull_request", "latest", "zigbuild", "cargo xwin")
    if any(value in workflow for value in forbidden):
        raise ReleaseCheckError("release workflow has an unsupported trigger or cross-build path")
    rehearsal = read(".github/workflows/native-ci.yml")
    rehearsal_lines = [line.strip() for line in rehearsal.splitlines()]
    for target, runner in TARGETS.items():
        if (
            rehearsal_lines.count(f"target: {target}") != 1
            or rehearsal_lines.count(f"- runner: {runner}") != 1
        ):
            raise ReleaseCheckError(f"rehearsal target mapping drifted: {target}")
    required_rehearsal = (
        "workflow_dispatch", "cargo test --locked --workspace --all-targets",
        "cargo build --locked --release -p immich-rs-cli", "host: ${{ matrix.target }}",
        "cargo build --locked --release -p immich-rs-web --bin immich-rs-web",
        "web_binary: target/release/immich-rs-web.exe",
        "fail-fast: false", "branches: [main]",
    )
    if any(value not in rehearsal for value in required_rehearsal):
        raise ReleaseCheckError("non-publishing rehearsal contract drifted")
    if "upload-artifact" in rehearsal or "RELEASE_SIGNING_PRIVATE_KEY" in rehearsal:
        raise ReleaseCheckError("native rehearsal must not publish or access signing material")
    package = read("scripts/package-release.py")
    for document in (
        "LICENSE", "NOTICE.md", "README.md", "SECURITY.md", "CHANGELOG.md",
        "docs/migration-from-immich-go.md", "docs/compatibility/phase5-archive.md",
        "docs/compatibility/phase7-production-https.md",
        "docs/compatibility/phase8-google-takeout-import.md",
        "docs/container.md", "docs/web-console-lan.md",
        "docs/web-console-resources.md", "docs/web-console-threat-model.md",
    ):
        if f'"{document}"' not in package:
            raise ReleaseCheckError(f"release package omits {document}")
    preflight = read("scripts/release-preflight.py")
    if '"git", "verify-tag"' not in preflight or "release-signing-key.asc" not in preflight:
        raise ReleaseCheckError("signed-tag preflight drifted")
    for document in (
        "docs/compatibility/phase7-production-https.md",
        "docs/compatibility/phase8-google-takeout-import.md",
        "docs/container.md", "docs/web-console-lan.md",
        "docs/web-console-resources.md", "docs/web-console-threat-model.md",
    ):
        if f'"{document}"' not in preflight:
            raise ReleaseCheckError(f"release preflight omits {document}")
    finalizer = read("scripts/finalize-release.py")
    if (
        "SHA256SUMS" not in finalizer
        or "immich-rs-build-provenance-v1" not in finalizer
        or "immich-rs-container-build-v1" not in finalizer
        or 'PRODUCTS = ("immich-rs", "immich-rs-web")' not in finalizer
    ):
        raise ReleaseCheckError("release checksum or provenance finalizer drifted")
    legacy_release = read(".gitea/workflows/release.yml")
    manual_container = read(".gitea/workflows/container.yml")
    for source, label in (
        (legacy_release, "legacy release"),
        (manual_container, "manual container"),
    ):
        if "--product immich-rs" not in source or "--product immich-rs-web" not in source:
            raise ReleaseCheckError(f"{label} omits one release product")
        if "docker push" in source or "--push" in source or ":latest" in source:
            raise ReleaseCheckError(f"{label} can publish an ordinary image")


def main() -> int:
    try:
        validate()
    except ReleaseCheckError as error:
        print(f"release pipeline check failed: {error}", file=sys.stderr)
        return 1
    print("release pipeline contract passed: rehearsal, five targets, SBOM, provenance, signing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
