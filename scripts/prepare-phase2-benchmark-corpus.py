#!/usr/bin/env python3
"""Create the portable standalone view of the synthetic Phase 2 corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent
WORKSPACE_PATTERN = re.compile(r"^immich-rs-(?:disposable|archive)\.[A-Za-z0-9._-]+$")
SOURCE_ROLES = {
    "clip.mp4": "standalone-video",
    "image.jpg": "standalone-image",
    "image.xmp": "xmp-sidecar",
    "live.jpg": "live-photo-image",
    "live.mov": "live-photo-video",
}
OUTPUT_ROLES = {
    "clip.mp4": "standalone-video",
    "image.jpg": "standalone-image",
    "image.xmp": "xmp-sidecar",
    "motion.mov": "standalone-video",
    "still.jpg": "standalone-image",
}


class CorpusError(RuntimeError):
    """The synthetic benchmark corpus contract is unsafe or inconsistent."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise CorpusError("source corpus manifest is unreadable") from error
    expected = {"upload_operations": 4, "xmp_sidecars": 1, "live_photo_pairs": 1}
    if (
        not isinstance(value, dict)
        or value.get("schema") != "phase2-corpus-v1"
        or value.get("fixture_id") != "synthetic-phase2-media-matrix"
        or value.get("synthetic") is not True
        or value.get("license") != "CC0-1.0"
        or not isinstance(value.get("generator"), dict)
        or value["generator"].get("recipe") != "ffmpeg-lavfi-apple-live-v2"
        or value.get("expected") != expected
        or not isinstance(value.get("files"), list)
    ):
        raise CorpusError("source corpus contract drifted")
    return value


def validate_workspace(paths: tuple[Path, ...]) -> Path:
    workspace = paths[0].parent
    if any(path.parent != workspace for path in paths[1:]) or not WORKSPACE_PATTERN.fullmatch(
        workspace.name
    ):
        raise CorpusError("benchmark corpus paths must remain inside one disposable workspace")
    if not workspace.is_dir() or workspace.is_symlink():
        raise CorpusError("disposable workspace must be a real directory")
    return workspace


def validate_source(source: Path, manifest: dict[str, Any]) -> None:
    observed: dict[str, tuple[str, int, str]] = {}
    for item in manifest["files"]:
        if not isinstance(item, dict):
            raise CorpusError("source corpus file entry is invalid")
        name, role, byte_len, digest = (
            item.get(key) for key in ("path", "role", "bytes", "sha256")
        )
        if (
            not isinstance(name, str)
            or re.fullmatch(r"[a-z0-9.-]+", name) is None
            or role != SOURCE_ROLES.get(name)
            or not isinstance(byte_len, int)
            or isinstance(byte_len, bool)
            or byte_len < 0
            or not isinstance(digest, str)
            or re.fullmatch(r"[0-9a-f]{64}", digest) is None
            or name in observed
        ):
            raise CorpusError("source corpus file entry is invalid")
        observed[name] = (role, byte_len, digest)
    if set(observed) != set(SOURCE_ROLES):
        raise CorpusError("source corpus file set drifted")
    for name, (_role, byte_len, digest) in observed.items():
        path = source / name
        if (
            not path.is_file()
            or path.is_symlink()
            or path.stat().st_size != byte_len
            or sha256(path) != digest
        ):
            raise CorpusError("source corpus digest drifted")


def validate_output(output: Path) -> None:
    output.mkdir(exist_ok=True)
    if not output.is_dir() or output.is_symlink():
        raise CorpusError("benchmark corpus output must be a real directory")
    for path in output.iterdir():
        if path.name not in OUTPUT_ROLES or not path.is_file() or path.is_symlink():
            raise CorpusError("benchmark corpus contains an undeclared entry")


def rewrite(source: Path, destination: Path, replacement: str) -> None:
    try:
        with source.open("rb") as input_handle, destination.open("wb") as output_handle:
            completed = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "rewrite-synthetic-identifier.py"),
                    "--from-identifier",
                    "synthetic-live-photo-v1",
                    "--to-identifier",
                    replacement,
                ],
                stdin=input_handle,
                stdout=output_handle,
                check=False,
                timeout=30,
            )
    except (OSError, subprocess.SubprocessError):
        destination.unlink(missing_ok=True)
        raise
    if completed.returncode != 0:
        destination.unlink(missing_ok=True)
        raise CorpusError("synthetic identifier rewrite failed")


def materialize(source: Path, output: Path) -> list[dict[str, Any]]:
    validate_output(output)
    for name in ("clip.mp4", "image.jpg", "image.xmp"):
        shutil.copyfile(source / name, output / name)
    rewrite(source / "live.mov", output / "motion.mov", "synthetic-video-only-v1")
    rewrite(source / "live.jpg", output / "still.jpg", "synthetic-still-only-v1")
    return [
        {
            "path": name,
            "role": role,
            "bytes": (output / name).stat().st_size,
            "sha256": sha256(output / name),
        }
        for name, role in OUTPUT_ROLES.items()
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--output-manifest", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        source = arguments.source.resolve(strict=True)
        source_manifest = arguments.source_manifest.resolve(strict=True)
        output = arguments.output.resolve(strict=False)
        output_manifest = arguments.output_manifest.resolve(strict=False)
        validate_workspace((source, source_manifest, output, output_manifest))
        if output_manifest == output or output in output_manifest.parents:
            raise CorpusError("output manifest must remain outside the media source")
        manifest = load_manifest(source_manifest)
        validate_source(source, manifest)
        files = materialize(source, output)
        report = {
            "schema": "phase2-benchmark-corpus-v1",
            "fixture_id": "synthetic-phase2-standalone-matrix",
            "synthetic": True,
            "license": "CC0-1.0",
            "derived_from": {
                "schema": "phase2-corpus-v1",
                "manifest_sha256": sha256(source_manifest),
                "mapping": "live media renamed and assigned distinct fixed-size synthetic identifiers",
            },
            "files": files,
            "expected": {
                "upload_operations": 4,
                "xmp_sidecars": 1,
                "live_photo_pairs": 0,
                "visible_assets": 4,
                "live_photo_links": 0,
            },
        }
        output_manifest.write_text(
            json.dumps(report, indent=2) + "\n", encoding="utf-8"
        )
    except (CorpusError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"benchmark corpus preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
