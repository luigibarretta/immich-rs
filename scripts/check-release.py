#!/usr/bin/env python3
"""Validate the fail-closed native release-candidate pipeline contract."""

from __future__ import annotations

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": "docker",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-apple-darwin": "macos-x64",
    "aarch64-apple-darwin": "macos-arm64",
    "x86_64-pc-windows-msvc": "windows-x64",
}


class ReleaseCheckError(ValueError):
    """The release workflow no longer satisfies ADR-0014/ADR-0025."""


def read(relative: str) -> str:
    try:
        return (ROOT / relative).read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ReleaseCheckError(f"cannot read {relative}") from error


def validate() -> None:
    workflow = read(".gitea/workflows/release.yml")
    for target, runner in TARGETS.items():
        if workflow.count(f"target: {target}") != 1 or workflow.count(f"runner: {runner}") != 1:
            raise ReleaseCheckError(f"native target mapping drifted: {target}")
    macos_host_python = (
        "runner: macos-x64\n            target: x86_64-apple-darwin\n"
        "            binary: target/release/immich-rs\n            python: python3.12\n"
        "            setup_python: false",
        "runner: macos-arm64\n            target: aarch64-apple-darwin\n"
        "            binary: target/release/immich-rs\n            python: python3.12\n"
        "            setup_python: false",
    )
    if any(value not in workflow for value in macos_host_python):
        raise ReleaseCheckError("macOS native jobs must use the validated host Python")
    windows_host_python = (
        "runner: windows-x64\n            target: x86_64-pc-windows-msvc\n"
        "            binary: target/release/immich-rs.exe\n            python: py -3.12\n"
        "            setup_python: false"
    )
    if windows_host_python not in workflow:
        raise ReleaseCheckError("Windows native jobs must use the validated host Python")
    required = (
        'tags: ["v*-rc.*"]',
        "cargo-cyclonedx@0.5.9",
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
        "cancel-in-progress: false",
        "if: ${{ matrix.setup_python }}",
        "${{ matrix.python }} scripts/package-release.py",
        "shell: powershell",
        'py -3.12 scripts/check-pe.py "${{ matrix.binary }}"',
        "check-production-evidence.py",
        "check-phase8-evidence.py",
        "check-phase8-benchmark.py",
    )
    if any(value not in workflow for value in required):
        raise ReleaseCheckError("release identity, SBOM or signing gate drifted")
    forbidden = ("workflow_dispatch", "pull_request", "latest", "zigbuild", "cargo xwin")
    if any(value in workflow for value in forbidden):
        raise ReleaseCheckError("release workflow has an unsupported trigger or cross-build path")
    package = read("scripts/package-release.py")
    for document in (
        "LICENSE", "NOTICE.md", "README.md", "SECURITY.md", "CHANGELOG.md",
        "docs/migration-from-immich-go.md", "docs/compatibility/phase5-archive.md",
        "docs/compatibility/phase7-production-https.md",
        "docs/compatibility/phase8-google-takeout-import.md",
    ):
        if f'"{document}"' not in package:
            raise ReleaseCheckError(f"release package omits {document}")
    preflight = read("scripts/release-preflight.py")
    if '"git", "verify-tag"' not in preflight or "release-signing-key.asc" not in preflight:
        raise ReleaseCheckError("signed-tag preflight drifted")
    for document in (
        "docs/compatibility/phase7-production-https.md",
        "docs/compatibility/phase8-google-takeout-import.md",
    ):
        if f'"{document}"' not in preflight:
            raise ReleaseCheckError(f"release preflight omits {document}")
    finalizer = read("scripts/finalize-release.py")
    if (
        "SHA256SUMS" not in finalizer
        or "immich-rs-build-provenance-v1" not in finalizer
        or "immich-rs-container-build-v1" not in finalizer
    ):
        raise ReleaseCheckError("release checksum or provenance finalizer drifted")


def main() -> int:
    try:
        validate()
    except ReleaseCheckError as error:
        print(f"release pipeline check failed: {error}", file=sys.stderr)
        return 1
    print("release pipeline contract passed: five native targets, SBOM, provenance, signing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
