#!/usr/bin/env python3
"""Materialize one bounded synthetic fixture as a directory or split ZIP view."""

from __future__ import annotations

import argparse
import binascii
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import zipfile
import zlib

SCHEMAS = {
    "fixture-manifest-v1": "1",
    "fixture-manifest-v2": "2",
    "fixture-manifest-v3": "3",
}


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
    schema = manifest.get("schema") if isinstance(manifest, dict) else None
    if schema not in SCHEMAS:
        raise FixtureError(f"manifest schema must be one of {sorted(SCHEMAS)}")
    provenance = manifest.get("provenance")
    expected_provenance = {
        "kind": "synthetic",
        "generator": "scripts/materialize-fixture.py",
        "generator_version": SCHEMAS[schema],
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
    if schema in {"fixture-manifest-v2", "fixture-manifest-v3"}:
        _validate_archive_views(manifest, set(path_strings))
    return manifest


def _validate_archive_views(manifest: dict[str, object], file_paths: set[str]) -> None:
    views = manifest.get("archive_views")
    if not isinstance(views, list) or not views:
        raise FixtureError("v2 fixture must declare archive_views")
    names: set[str] = set()
    for view in views:
        if not isinstance(view, dict) or not isinstance(view.get("name"), str):
            raise FixtureError("archive view must have a string name")
        name = view["name"]
        if not name or name in names:
            raise FixtureError("archive view names must be non-empty and unique")
        names.add(name)
        parts = view.get("parts")
        if not isinstance(parts, list) or not parts:
            raise FixtureError("archive view must contain parts")
        assigned: list[str] = []
        part_names: list[str] = []
        for part in parts:
            if not isinstance(part, dict):
                raise FixtureError("archive part must be an object")
            archive_path = _portable_relative_path(part.get("path"))
            paths = part.get("files")
            if archive_path.parent != Path(".") or archive_path.suffix.lower() != ".zip":
                raise FixtureError("archive part must be a root-level ZIP")
            if not isinstance(paths, list) or not paths:
                raise FixtureError("archive part must contain files")
            part_names.append(archive_path.as_posix())
            portable_paths = [_portable_relative_path(path).as_posix() for path in paths]
            if portable_paths != sorted(portable_paths):
                raise FixtureError("archive part files must be sorted")
            assigned.extend(portable_paths)
        if part_names != sorted(part_names) or len(part_names) != len(set(part_names)):
            raise FixtureError("archive parts must be unique and sorted")
        if len(assigned) != len(set(assigned)) or set(assigned) != file_paths:
            raise FixtureError("each archive view must partition all fixture files exactly once")


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
        with destination.open("xb") as handle:
            handle.write(content.encode("utf-8"))
    elif recipe == "synthetic_png":
        with destination.open("xb") as handle:
            handle.write(_synthetic_png(item))
    elif recipe == "synthetic_padded_png":
        _write_padded_png(destination, item)
    elif recipe == "synthetic_isobmff":
        with destination.open("xb") as handle:
            handle.write(_synthetic_isobmff(item))
    elif recipe == "symlink":
        target = item.get("target")
        target_path = _portable_relative_path(target)
        os.symlink(target_path.as_posix(), destination)
    else:
        raise FixtureError(f"unsupported fixture recipe: {recipe!r}")


def _materialize_files(manifest: dict[str, object], output: Path) -> None:
    files = manifest["files"]
    if not isinstance(files, list):
        raise FixtureError("fixture files must be a list")
    for item in files:
        if not isinstance(item, dict):
            raise FixtureError("fixture file entry must be an object")
        _materialize_item(output, item)


def _write_archive(source: Path, destination: Path, paths: list[object]) -> None:
    with zipfile.ZipFile(destination, "x") as archive:
        for raw_path in paths:
            relative = _portable_relative_path(raw_path)
            source_path = source.joinpath(relative)
            if not source_path.is_file() or source_path.is_symlink():
                raise FixtureError("archive views support only generated regular files")
            info = zipfile.ZipInfo(relative.as_posix(), date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            with source_path.open("rb") as reader, archive.open(info, "w") as writer:
                shutil.copyfileobj(reader, writer, length=64 * 1_024)


def materialize(manifest_path: Path, output: Path, archive_view: str | None = None) -> None:
    """Materialize a manifest into a new or empty directory."""
    manifest = load_manifest(manifest_path)
    if output.exists() and any(output.iterdir()):
        raise FixtureError("fixture output directory must be empty")
    output.mkdir(parents=True, exist_ok=True)
    if archive_view is None:
        _materialize_files(manifest, output)
        return
    views = manifest.get("archive_views")
    selected = next(
        (view for view in views if isinstance(view, dict) and view.get("name") == archive_view),
        None,
    ) if isinstance(views, list) else None
    if not isinstance(selected, dict) or not isinstance(selected.get("parts"), list):
        raise FixtureError(f"unknown archive view: {archive_view}")
    staging = output / ".fixture-staging"
    staging.mkdir()
    try:
        _materialize_files(manifest, staging)
        for part in selected["parts"]:
            if not isinstance(part, dict) or not isinstance(part.get("files"), list):
                raise FixtureError("invalid archive part")
            archive_path = _portable_relative_path(part.get("path"))
            _write_archive(staging, output / archive_path, part["files"])
    finally:
        shutil.rmtree(staging)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--archive-view")
    arguments = parser.parse_args()
    try:
        materialize(arguments.manifest, arguments.output, arguments.archive_view)
    except (FixtureError, OSError) as error:
        print(f"fixture materialization failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
