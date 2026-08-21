#!/usr/bin/env python3
"""Validate the complete five-target release set and emit SHA256SUMS."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

TARGETS = {
    "x86_64-unknown-linux-gnu": "tar.gz",
    "aarch64-unknown-linux-gnu": "tar.gz",
    "x86_64-apple-darwin": "tar.gz",
    "aarch64-apple-darwin": "tar.gz",
    "x86_64-pc-windows-msvc": "zip",
}
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*")


class FinalizeError(RuntimeError):
    """The native release set is incomplete or inconsistent."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def finalize(root: Path, version: str, revision: str, output: Path) -> None:
    if VERSION.fullmatch(version) is None or re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise FinalizeError("release version or revision is invalid")
    expected = []
    for target, extension in TARGETS.items():
        stem = f"immich-rs-{version}-{target}"
        expected.extend(
            (
                root / f"{stem}.{extension}",
                root / f"{stem}.sbom.cdx.json",
                root / f"{stem}.provenance.json",
            )
        )
    if output.exists() or any(not path.is_file() or path.is_symlink() for path in expected):
        raise FinalizeError("release artifacts are missing, linked or output already exists")
    extras = sorted(path.name for path in root.iterdir() if path.is_file() and path not in expected)
    if extras:
        raise FinalizeError("release directory contains unexpected files")
    for path in expected:
        if path.stat().st_size <= 0 or path.stat().st_size > 256 * 1_024 * 1_024:
            raise FinalizeError("release artifact size is outside its bound")
        if path.name.endswith(".sbom.cdx.json"):
            value = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(value, dict) or value.get("bomFormat") != "CycloneDX":
                raise FinalizeError("release SBOM is invalid")
        if path.name.endswith(".provenance.json"):
            value = json.loads(path.read_text(encoding="utf-8"))
            if (
                not isinstance(value, dict)
                or value.get("schema") != "immich-rs-build-provenance-v1"
                or value.get("version") != version
                or value.get("source_revision") != revision
            ):
                raise FinalizeError("release provenance identity drift")
    lines = [f"{sha256(path)}  {path.name}" for path in sorted(expected, key=lambda item: item.name)]
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        finalize(arguments.input.resolve(), arguments.version, arguments.revision, arguments.output)
    except (FinalizeError, OSError, UnicodeError, ValueError, json.JSONDecodeError) as error:
        print(f"release finalization failed: {error}", file=sys.stderr)
        return 1
    print(f"release checksums ready: {arguments.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
