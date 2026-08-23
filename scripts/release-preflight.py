#!/usr/bin/env python3
"""Fail closed unless a signed release-candidate tag is ready to build."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys
import tomllib

TAG = re.compile(r"v(?P<version>[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*)")


class ReleaseError(RuntimeError):
    """A release-candidate prerequisite is absent or inconsistent."""


def command(root: Path, arguments: list[str]) -> str:
    try:
        completed = subprocess.run(
            arguments,
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise ReleaseError(f"release command failed: {arguments[0]}") from error
    return completed.stdout.strip()


def tag_version(tag: str) -> str:
    matched = TAG.fullmatch(tag)
    if matched is None:
        raise ReleaseError("release tag must be v<semver>-rc.<positive-number>")
    return matched.group("version")


def workspace_version(root: Path) -> str:
    try:
        manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
        value = manifest["workspace"]["package"]["version"]
    except (OSError, UnicodeError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        raise ReleaseError("workspace version is unavailable") from error
    if not isinstance(value, str):
        raise ReleaseError("workspace version must be a string")
    return value


def validate(root: Path, tag: str, revision: str) -> None:
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise ReleaseError("release revision must be an exact commit")
    if workspace_version(root) != tag_version(tag):
        raise ReleaseError("release tag and Cargo workspace version differ")
    public_key = root / "docs/release-signing-key.asc"
    if not public_key.is_file() or not public_key.read_text(encoding="utf-8").startswith(
        "-----BEGIN PGP PUBLIC KEY BLOCK-----"
    ):
        raise ReleaseError("committed release signing public key is unavailable")
    if command(root, ["git", "status", "--porcelain"]):
        raise ReleaseError("release worktree is not clean")
    if command(root, ["git", "rev-parse", "HEAD"]) != revision:
        raise ReleaseError("release checkout does not match the workflow revision")
    if command(root, ["git", "rev-list", "-n", "1", tag]) != revision:
        raise ReleaseError("release tag does not resolve to the workflow revision")
    command(root, ["git", "verify-tag", tag])
    required = (
        "CHANGELOG.md", "LICENSE", "NOTICE.md", "README.md", "SECURITY.md",
        "docs/migration-from-immich-go.md", "docs/release-process.md",
        "docs/compatibility/phase5-archive.md",
        "docs/compatibility/phase7-production-https.md",
        "docs/compatibility/phase8-google-takeout-import.md",
    )
    missing = [name for name in required if not (root / name).is_file()]
    if missing:
        raise ReleaseError("release documentation is incomplete")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parent.parent)
    arguments = parser.parse_args()
    try:
        validate(arguments.repository.resolve(), arguments.tag, arguments.revision)
    except (ReleaseError, OSError, UnicodeError) as error:
        print(f"release preflight failed: {error}", file=sys.stderr)
        return 1
    print(f"release preflight passed: {arguments.tag} at {arguments.revision}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
