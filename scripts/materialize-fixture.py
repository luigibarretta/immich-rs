#!/usr/bin/env python3
"""Materialize one bounded synthetic fixture from fixture-manifest-v1."""

from __future__ import annotations

import argparse
import binascii
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import zlib

SCHEMA = "fixture-manifest-v1"
GENERATOR_VERSION = "1"


class FixtureError(ValueError):
    """A fixture is unsafe, malformed or unsupported."""


def _portable_relative_path(raw: object) -> Path:
    if not isinstance(raw, str) or not raw or "\\" in raw or "\x00" in raw:
        raise FixtureError("fixture path must be a non-empty portable string")
    path = Path(raw)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise FixtureError(f"unsafe fixture path: {raw!r}")
    return path


def load_manifest(path: Path) -> dict[str, object]:
    """Load and minimally validate a v1 manifest without external packages."""
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise FixtureError(f"cannot read fixture manifest: {error}") from error
    if not isinstance(manifest, dict) or manifest.get("schema") != SCHEMA:
        raise FixtureError(f"manifest schema must be {SCHEMA}")
    provenance = manifest.get("provenance")
    expected_provenance = {
        "kind": "synthetic",
        "generator": "scripts/materialize-fixture.py",
        "generator_version": GENERATOR_VERSION,
        "license": "CC0-1.0",
    }
    if provenance != expected_provenance:
        raise FixtureError("fixture provenance is missing or unsupported")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise FixtureError("fixture must declare at least one file")
    paths = [_portable_relative_path(item.get("path") if isinstance(item, dict) else None) for item in files]
    path_strings = [path.as_posix() for path in paths]
    if path_strings != sorted(path_strings) or len(path_strings) != len(set(path_strings)):
        raise FixtureError("fixture paths must be unique and strictly sorted")
    return manifest


def _chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", binascii.crc32(kind + data) & 0xFFFFFFFF)


def _synthetic_png(item: dict[str, object]) -> bytes:
    width = item.get("width")
    height = item.get("height")
    rgb = item.get("rgb")
    if not isinstance(width, int) or not isinstance(height, int) or not (1 <= width <= 64 and 1 <= height <= 64):
        raise FixtureError("synthetic_png dimensions must be integers in 1..64")
    if not isinstance(rgb, list) or len(rgb) != 3 or not all(isinstance(value, int) and 0 <= value <= 255 for value in rgb):
        raise FixtureError("synthetic_png rgb must contain three bytes")
    pixel = bytes(rgb)
    rows = b"".join(b"\x00" + pixel * width for _ in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", header) + _chunk(b"IDAT", zlib.compress(rows, 9)) + _chunk(b"IEND", b"")


def _synthetic_isobmff(item: dict[str, object]) -> bytes:
    marker = item.get("marker", "synthetic-motion")
    if not isinstance(marker, str) or not marker.isascii() or not (1 <= len(marker) <= 64):
        raise FixtureError("synthetic_isobmff marker must be bounded ASCII")
    ftyp_payload = b"qt  " + struct.pack(">I", 0) + b"qt  "
    ftyp = struct.pack(">I4s", len(ftyp_payload) + 8, b"ftyp") + ftyp_payload
    marker_bytes = marker.encode("ascii")
    free = struct.pack(">I4s", len(marker_bytes) + 8, b"free") + marker_bytes
    return ftyp + free


def _write_padded_png(destination: Path, item: dict[str, object]) -> None:
    """Stream a valid tiny PNG plus deterministic synthetic padding."""
    total_bytes = item.get("bytes")
    seed = item.get("seed")
    if not isinstance(total_bytes, int) or not (1_048_576 <= total_bytes <= 16_777_216):
        raise FixtureError("synthetic_padded_png bytes must be in 1048576..16777216")
    if not isinstance(seed, str) or not seed.isascii() or not (1 <= len(seed) <= 64):
        raise FixtureError("synthetic_padded_png seed must be bounded ASCII")
    prefix = _synthetic_png(item)
    if len(prefix) >= total_bytes:
        raise FixtureError("synthetic_padded_png bytes must exceed the PNG prefix")
    block = hashlib.sha256(seed.encode("ascii")).digest() * 2_048
    remaining = total_bytes - len(prefix)
    with destination.open("xb") as handle:
        handle.write(prefix)
        while remaining:
            chunk = block[: min(remaining, len(block))]
            handle.write(chunk)
            remaining -= len(chunk)


def _materialize_item(root: Path, item: dict[str, object]) -> None:
    relative = _portable_relative_path(item.get("path"))
    destination = root.joinpath(relative)
    destination.parent.mkdir(parents=True, exist_ok=True)
    recipe = item.get("recipe")
    if recipe == "text":
        content = item.get("content")
        if not isinstance(content, str) or len(content.encode("utf-8")) > 65_536:
            raise FixtureError("text fixture content must be bounded UTF-8")
        destination.write_text(content, encoding="utf-8", newline="\n")
    elif recipe == "synthetic_png":
        destination.write_bytes(_synthetic_png(item))
    elif recipe == "synthetic_padded_png":
        _write_padded_png(destination, item)
    elif recipe == "synthetic_isobmff":
        destination.write_bytes(_synthetic_isobmff(item))
    elif recipe == "symlink":
        target = item.get("target")
        target_path = _portable_relative_path(target)
        os.symlink(target_path.as_posix(), destination)
    else:
        raise FixtureError(f"unsupported fixture recipe: {recipe!r}")


def materialize(manifest_path: Path, output: Path) -> None:
    """Materialize a manifest into a new or empty directory."""
    manifest = load_manifest(manifest_path)
    if output.exists() and any(output.iterdir()):
        raise FixtureError("fixture output directory must be empty")
    output.mkdir(parents=True, exist_ok=True)
    files = manifest["files"]
    if not isinstance(files, list):
        raise FixtureError("fixture files must be a list")
    for item in files:
        if not isinstance(item, dict):
            raise FixtureError("fixture file entry must be an object")
        _materialize_item(output, item)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    arguments = parser.parse_args()
    try:
        materialize(arguments.manifest, arguments.output)
    except (FixtureError, OSError) as error:
        print(f"fixture materialization failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
