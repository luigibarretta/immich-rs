#!/usr/bin/env python3
"""Run a bounded read-only soak over a large reproducible synthetic library."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import statistics
import sys
import tempfile
from types import ModuleType
from typing import Any

ASSET_COUNT = 2_500
MEDIA_BYTES = 512 * 1_024
ASSETS_PER_ALBUM = 100
BUFFER_BYTES = 64 * 1_024
MAX_RSS_BYTES = 256 * 1_024 * 1_024


class SoakError(RuntimeError):
    """The synthetic soak violated a reproducibility or resource invariant."""


def load_metrics(path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location("soak_metrics", path)
    if spec is None or spec.loader is None:
        raise SoakError("process sampler is unavailable")
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


def media_block(index: int) -> bytes:
    seed = hashlib.sha256(f"immich-rs-phase6-asset-{index:05d}".encode()).digest()
    return (seed * (BUFFER_BYTES // len(seed) + 1))[:BUFFER_BYTES]


def materialize(root: Path, asset_count: int, media_bytes: int) -> dict[str, Any]:
    if asset_count <= 0 or media_bytes <= 0 or media_bytes % BUFFER_BYTES != 0:
        raise SoakError("synthetic corpus dimensions are invalid")
    corpus_digest = hashlib.sha256()
    allocated_bytes = 0
    sidecar_bytes = 0
    for index in range(asset_count):
        album = root / "Takeout" / "Google Photos" / f"Synthetic Album {index // ASSETS_PER_ALBUM:03d}"
        album.mkdir(parents=True, exist_ok=True)
        name = f"asset-{index:05d}.jpg"
        media = album / name
        content_digest = hashlib.sha256()
        block = media_block(index)
        with media.open("xb") as output:
            for _ in range(media_bytes // len(block)):
                output.write(block)
                content_digest.update(block)
        sidecar = album / f"{name}.json"
        document = {
            "description": "synthetic phase six soak",
            "photoTakenTime": {"timestamp": str(1_700_000_000 + index)},
            "title": name,
        }
        encoded = (json.dumps(document, separators=(",", ":"), sort_keys=True) + "\n").encode()
        with sidecar.open("xb") as output:
            output.write(encoded)
        sidecar_bytes += len(encoded)
        relative = media.relative_to(root).as_posix().encode()
        corpus_digest.update(relative)
        corpus_digest.update(content_digest.digest())
        corpus_digest.update(hashlib.sha256(encoded).digest())
        allocated_bytes += media.stat().st_blocks * 512 + sidecar.stat().st_blocks * 512
    return {
        "assets": asset_count,
        "sidecars": asset_count,
        "logical_media_bytes": asset_count * media_bytes,
        "logical_sidecar_bytes": sidecar_bytes,
        "logical_source_bytes": asset_count * media_bytes + sidecar_bytes,
        "allocated_bytes": allocated_bytes,
        "corpus_sha256": corpus_digest.hexdigest(),
        "generator": "phase6-deterministic-block-v1",
    }


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
    metrics: ModuleType,
    index: int,
) -> tuple[dict[str, Any], dict[str, int], str]:
    try:
        completed, measured = metrics.run_command(
            [
                str(binary), "plan", "google-takeout", "--label", "synthetic-phase6-soak",
                "--buffer-bytes", str(BUFFER_BYTES), "--max-entries", "6000",
                "--max-directory-entries", "256", str(source),
            ],
            workspace,
            child_environment(),
            1_800,
        )
    except RuntimeError as error:
        raise SoakError("read-only synthetic soak could not be measured") from error
    if completed.returncode != 0 or completed.stderr:
        raise SoakError("read-only synthetic soak process failed")
    try:
        plan = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise SoakError("read-only synthetic soak returned invalid JSON") from error
    summary = plan.get("summary")
    if plan.get("schema_version") != 2 or not isinstance(summary, dict):
        raise SoakError("synthetic soak plan contract drifted")
    normalized = {
        "assets": summary.get("assets"),
        "sidecars": summary.get("sidecars"),
        "bytes_read": summary.get("bytes_read"),
    }
    if any(not isinstance(value, int) or isinstance(value, bool) for value in normalized.values()):
        raise SoakError("synthetic soak summary counters are invalid")
    measured["sample"] = index
    return measured, normalized, hashlib.sha256(completed.stdout.encode()).hexdigest()


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    if re.fullmatch(r"[0-9a-f]{40}", arguments.source_revision) is None:
        raise SoakError("source revision must be an exact commit")
    binary = arguments.binary.resolve()
    metrics_path = arguments.metrics.resolve()
    if not binary.is_file() or not metrics_path.is_file():
        raise SoakError("soak tooling is unavailable")
    sampler = load_metrics(metrics_path)
    temporary_path: Path | None = None
    with tempfile.TemporaryDirectory(prefix=".immich-rs-soak-", dir=arguments.workspace) as temporary:
        temporary_path = Path(temporary)
        source = temporary_path / "source"
        source.mkdir()
        fixture = materialize(source, ASSET_COUNT, MEDIA_BYTES)
        if fixture["allocated_bytes"] < fixture["logical_source_bytes"]:
            raise SoakError("synthetic corpus was not fully allocated")
        os.sync()
        measurements = []
        summaries = []
        plan_digests = []
        for index in range(4):
            measured, summary, plan_digest = run_sample(
                binary, source, temporary_path, sampler, index
            )
            if index > 0:
                measurements.append(measured)
                summaries.append(summary)
                plan_digests.append(plan_digest)
        expected = {
            "assets": ASSET_COUNT,
            "sidecars": ASSET_COUNT,
            "bytes_read": fixture["logical_source_bytes"],
        }
        if any(summary != expected for summary in summaries):
            raise SoakError("synthetic soak plan counters drifted")
        if len(set(plan_digests)) != 1:
            raise SoakError("unchanged synthetic corpus produced nondeterministic plans")
        peak_rss = max(value["peak_rss_bytes"] for value in measurements)
        if peak_rss > MAX_RSS_BYTES:
            raise SoakError("synthetic soak exceeded its client RSS budget")
        report = {
            "schema": "phase6-synthetic-soak-v1",
            "fixture": fixture,
            "plan_summary": expected,
            "normalized_plan_sha256": plan_digests[0],
            "methodology": {
                "buffer_bytes": BUFFER_BYTES,
                "warmups": 1,
                "retained_samples": 3,
                "cache_state": "one warmup before retained page-cached samples",
                "scope": "read-only Google Takeout scan and normalized plan",
            },
            "raw_samples": measurements,
            "aggregate": {
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
                "binary_sha256": sha256(binary),
            },
        }
    report["cleanup_verified"] = temporary_path is not None and not temporary_path.exists()
    if not report["cleanup_verified"]:
        raise SoakError("synthetic soak workspace cleanup was not verified")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--metrics", type=Path, required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        report = run(arguments)
        arguments.output.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (SoakError, OSError, UnicodeError, ValueError) as error:
        print(f"synthetic soak failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
