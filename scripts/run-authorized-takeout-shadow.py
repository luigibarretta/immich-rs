#!/usr/bin/env python3
"""Run a privacy-safe read-only shadow scan over an authorized Takeout subset."""

from __future__ import annotations

import argparse
from collections import defaultdict
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import statistics
import subprocess
import sys
import tempfile
from types import ModuleType
from typing import Any
from zipfile import ZipFile, ZipInfo

MEDIA_SUFFIXES = {
    ".3gp", ".avi", ".gif", ".heic", ".jpeg", ".jpg", ".m4v",
    ".mkv", ".mov", ".mp4", ".png", ".webp",
}
TARGET_BYTES = 512 * 1_024 * 1_024
MAX_SUBSET_BYTES = 2 * 1_024 * 1_024 * 1_024
MAX_SUBSET_ENTRIES = 10_000
MAX_RSS_BYTES = 256 * 1_024 * 1_024


class ShadowError(RuntimeError):
    """The authorized read-only shadow run failed closed."""


def load_metrics(path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location("shadow_metrics", path)
    if spec is None or spec.loader is None:
        raise ShadowError("process sampler is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def select_group(archive: ZipFile) -> tuple[str, list[ZipInfo]]:
    groups: dict[str, list[ZipInfo]] = defaultdict(list)
    for entry in archive.infolist():
        parts = PurePosixPath(entry.filename).parts
        if len(parts) >= 4 and parts[:2] == ("Takeout", "Google Photos"):
            groups[parts[2]].append(entry)
    candidates = []
    for group, entries in groups.items():
        byte_len = sum(entry.file_size for entry in entries if not entry.is_dir())
        media = sum(
            not entry.is_dir()
            and PurePosixPath(entry.filename).suffix.casefold() in MEDIA_SUFFIXES
            for entry in entries
        )
        if (
            media >= 100
            and len(entries) <= MAX_SUBSET_ENTRIES
            and byte_len <= MAX_SUBSET_BYTES
        ):
            candidates.append((abs(byte_len - TARGET_BYTES), group, entries))
    if not candidates:
        raise ShadowError("no bounded Takeout subset satisfies the shadow contract")
    _, group, entries = min(candidates, key=lambda value: (value[0], value[1]))
    return group, entries


def safe_target(root: Path, entry: ZipInfo) -> Path:
    path = PurePosixPath(entry.filename)
    if (
        path.is_absolute()
        or ".." in path.parts
        or path.parts[:2] != ("Takeout", "Google Photos")
        or (entry.external_attr >> 16) & 0o170000 == 0o120000
    ):
        raise ShadowError("Takeout subset contains an unsafe archive entry")
    return root.joinpath(*path.parts)


def extract_group(archive: ZipFile, entries: list[ZipInfo], destination: Path) -> dict[str, int]:
    media = 0
    files = 0
    byte_len = 0
    buffer = bytearray(1_048_576)
    for entry in sorted(entries, key=lambda value: value.filename):
        target = safe_target(destination, entry)
        if entry.is_dir():
            target.mkdir(parents=True, exist_ok=True)
            continue
        files += 1
        byte_len += entry.file_size
        if files > MAX_SUBSET_ENTRIES or byte_len > MAX_SUBSET_BYTES:
            raise ShadowError("Takeout subset exceeded its extraction bound")
        media += PurePosixPath(entry.filename).suffix.casefold() in MEDIA_SUFFIXES
        target.parent.mkdir(parents=True, exist_ok=True)
        with archive.open(entry) as source, target.open("xb") as output:
            while read := source.readinto(buffer):
                output.write(memoryview(buffer)[:read])
    return {"archive_entries": files, "archive_bytes": byte_len, "media_candidates": media}


def child_environment() -> dict[str, str]:
    return {
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "TZ": "UTC",
    }


def run_sample(
    binary: Path,
    source: Path,
    workspace: Path,
    index: int,
    metrics: ModuleType,
) -> tuple[dict[str, Any], dict[str, Any]]:
    completed, measured = metrics.run_command(
        [
            str(binary), "plan", "google-takeout", "--label", "authorized-shadow",
            "--buffer-bytes", "65536", "--max-entries", "10000", str(source),
        ],
        workspace,
        child_environment(),
        1_800,
    )
    if completed.returncode != 0 or completed.stderr:
        raise ShadowError("read-only Takeout shadow process failed")
    try:
        plan = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ShadowError("read-only Takeout shadow returned invalid JSON") from error
    summary = plan.get("summary")
    if not isinstance(summary, dict) or plan.get("schema_version") != 2:
        raise ShadowError("read-only Takeout plan contract drifted")
    measured["sample"] = index
    return measured, summary


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    archives = sorted(path for path in arguments.takeout_root.rglob("*.zip") if path.is_file())
    if len(archives) != 1:
        raise ShadowError("authorized Takeout root must contain exactly one ZIP")
    if not arguments.binary.is_file() or not arguments.metrics.is_file():
        raise ShadowError("shadow tooling is unavailable")
    if re.fullmatch(r"[0-9a-f]{40}", arguments.source_revision) is None:
        raise ShadowError("source revision must be an exact commit")
    sampler = load_metrics(arguments.metrics)
    temporary_path: Path | None = None
    with tempfile.TemporaryDirectory(
        prefix=".immich-rs-shadow-", dir=arguments.takeout_root
    ) as temporary:
        temporary_path = Path(temporary)
        source = temporary_path / "source"
        source.mkdir()
        with ZipFile(archives[0]) as archive:
            _, entries = select_group(archive)
            subset = extract_group(archive, entries, source)
        measurements = []
        summaries = []
        for index in range(4):
            measured, summary = run_sample(
                arguments.binary, source, temporary_path, index, sampler
            )
            if index > 0:
                measurements.append(measured)
                summaries.append(summary)
        if any(summary != summaries[0] for summary in summaries[1:]):
            raise ShadowError("unchanged Takeout subset produced nondeterministic summaries")
        peak_rss = max(value["peak_rss_bytes"] for value in measurements)
        if peak_rss > MAX_RSS_BYTES:
            raise ShadowError("Takeout shadow exceeded its client RSS budget")
        summary = summaries[0]
        report = {
            "schema": "authorized-takeout-shadow-v1",
            "authorization": "explicit user-provided private read-only corpus",
            "privacy": "aggregate counters only; no paths, names, metadata or content digests",
            "fixture": subset,
            "plan_summary": summary,
            "deterministic_retained_runs": len(measurements),
            "metrics": {
                "wall_time_seconds": [value["wall_time_seconds"] for value in measurements],
                "median_wall_time_seconds": statistics.median(
                    value["wall_time_seconds"] for value in measurements
                ),
                "peak_rss_bytes": peak_rss,
                "peak_open_file_descriptors": max(
                    value["peak_open_file_descriptors"] for value in measurements
                ),
            },
            "environment": {
                "source_revision": arguments.source_revision,
                "system": platform.system(),
                "architecture": platform.machine(),
                "logical_cpus": os.cpu_count(),
                "hostname": "<REDACTED_HOST>",
                "binary_sha256": sha256(arguments.binary),
            },
        }
    report["cleanup_verified"] = temporary_path is not None and not temporary_path.exists()
    if not report["cleanup_verified"]:
        raise ShadowError("private shadow workspace cleanup was not verified")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--takeout-root", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--metrics", type=Path, required=True)
    parser.add_argument("--source-revision", required=True)
    arguments = parser.parse_args()
    try:
        print(json.dumps(run(arguments), sort_keys=True))
    except (ShadowError, OSError, UnicodeError, ValueError) as error:
        print(f"authorized Takeout shadow failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
