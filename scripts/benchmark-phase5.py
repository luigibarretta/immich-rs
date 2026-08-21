#!/usr/bin/env python3
"""Benchmark paired read-only archives on one disposable Immich server."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import tomllib
from types import ModuleType
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
BASELINE_PATH = REPOSITORY_ROOT / "tests/oracle/baseline.toml"
METRICS = (
    "wall_time_seconds",
    "user_cpu_seconds",
    "system_cpu_seconds",
    "peak_rss_bytes",
    "peak_open_file_descriptors",
    "characters_read",
    "characters_written",
    "storage_bytes_read",
    "storage_bytes_written",
    "logical_media_bytes_read",
    "logical_media_bytes_written",
)
SUM_METRICS = set(METRICS) - {"peak_rss_bytes", "peak_open_file_descriptors"}


class BenchmarkError(RuntimeError):
    """A Phase 5 paired benchmark invariant failed."""


def load_module(path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location("phase5_metrics", path)
    if spec is None or spec.loader is None:
        raise BenchmarkError("cannot load process sampler")
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


def tool_version(command: list[str]) -> str:
    completed = subprocess.run(
        command, check=True, capture_output=True, text=True, timeout=15
    )
    return (completed.stdout or completed.stderr).strip()


def environment(api_key: str | None = None) -> dict[str, str]:
    selected = {
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "TZ": "UTC",
    }
    if api_key is not None:
        selected["IMMICH_RS_API_KEY"] = api_key
    return selected


def fixture_identity(path: Path, source: Path) -> tuple[dict[str, Any], Counter[str], int]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema") != "phase2-corpus-v1" or value.get("synthetic") is not True:
        raise BenchmarkError("synthetic fixture contract drifted")
    hashes: Counter[str] = Counter()
    media_bytes = 0
    for entry in value.get("files", []):
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
            raise BenchmarkError("fixture entry is invalid")
        if entry.get("role") == "xmp-sidecar":
            continue
        media = source / entry["path"]
        if not media.is_file() or media.stat().st_size != entry.get("bytes"):
            raise BenchmarkError("fixture media length drifted")
        digest = sha256(media)
        if digest != entry.get("sha256"):
            raise BenchmarkError("fixture media digest drifted")
        hashes[digest] += 1
        media_bytes += media.stat().st_size
    if sum(hashes.values()) != 4:
        raise BenchmarkError("fixture must contain four original media files")
    return value, hashes, media_bytes


def verify_archive(destination: Path, expected: Counter[str], expected_bytes: int) -> dict[str, int]:
    observed: Counter[str] = Counter()
    total_bytes = 0
    for path in sorted(candidate for candidate in destination.rglob("*") if candidate.is_file()):
        digest = sha256(path)
        if digest in expected:
            observed[digest] += 1
            total_bytes += path.stat().st_size
    if observed != expected or total_bytes != expected_bytes:
        raise BenchmarkError("archived original-byte multiset differs from the fixture")
    return {"original_assets": sum(observed.values()), "media_bytes": total_bytes, "retries": 0}


def combine(values: list[dict[str, Any]]) -> dict[str, Any]:
    result = {}
    for field in METRICS[:-2]:
        samples = [value[field] for value in values]
        result[field] = sum(samples) if field in SUM_METRICS else max(samples)
    return result


def run_rust(
    binary: Path,
    endpoint: str,
    api_key: str,
    workspace: Path,
    metrics: ModuleType,
    expected: Counter[str],
    media_bytes: int,
) -> dict[str, Any]:
    manifest = workspace / "manifest.json"
    destination = workspace / "archive"
    planned, plan_metrics = metrics.run_command(
        [
            str(binary), "plan", "archive", "immich", "--server", endpoint,
            "--selection", "timeline", "--page-size", "100", "--max-assets", "8",
        ],
        workspace,
        environment(api_key),
        180,
    )
    if planned.returncode != 0:
        raise BenchmarkError("immich-rs archive planning failed")
    manifest.write_text(planned.stdout, encoding="utf-8")
    manifest.chmod(0o600)
    applied, apply_metrics = metrics.run_command(
        [
            str(binary), "apply", "archive", "--server", endpoint,
            "--manifest", str(manifest), "--destination", str(destination),
        ],
        workspace,
        environment(api_key),
        180,
    )
    if applied.returncode != 0:
        raise BenchmarkError("immich-rs archive apply failed")
    report = json.loads(applied.stdout)
    if report.get("downloaded") != 4 or report.get("retries") != 0:
        raise BenchmarkError("immich-rs archive report drifted")
    measured = combine([plan_metrics, apply_metrics])
    measured["logical_media_bytes_read"] = media_bytes
    measured["logical_media_bytes_written"] = media_bytes
    measured["operations"] = verify_archive(destination, expected, media_bytes)
    return measured


def run_go(
    binary: Path,
    endpoint: str,
    api_key: str,
    workspace: Path,
    metrics: ModuleType,
    expected: Counter[str],
    media_bytes: int,
) -> dict[str, Any]:
    destination = workspace / "archive"
    completed, measured = metrics.run_command(
        [
            str(binary), "archive", "from-immich", "--from-server", endpoint,
            "--from-api-key", api_key, "--from-pause-immich-jobs=false",
            "--concurrent-tasks", "1", "--log-level", "ERROR",
            "--write-to-folder", str(destination),
        ],
        workspace,
        environment(),
        180,
    )
    if completed.returncode != 0:
        details = (completed.stdout + "\n" + completed.stderr).strip()
        for sensitive in (api_key, endpoint, str(workspace)):
            details = details.replace(sensitive, "<REDACTED>")
        raise BenchmarkError(f"immich-go archive failed: {details[-2_000:]}")
    measured["logical_media_bytes_read"] = media_bytes
    measured["logical_media_bytes_written"] = media_bytes
    measured["operations"] = verify_archive(destination, expected, media_bytes)
    return measured


def aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result = {}
    for field in METRICS:
        values = sorted(sample[tool][field] for sample in samples)
        p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {
            "min": min(values),
            "median": statistics.median(values),
            "p95": values[p95_index],
            "max": max(values),
        }
    return result


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    if re.fullmatch(r"http://127\.0\.0\.1:[0-9]{1,5}", arguments.endpoint) is None:
        raise BenchmarkError("endpoint must be literal IPv4 loopback")
    if re.fullmatch(r"[0-9a-f]{40}", arguments.source_revision) is None:
        raise BenchmarkError("source revision must be exact")
    if not 2 <= arguments.samples <= 10 or not 0 <= arguments.warmups <= 2:
        raise BenchmarkError("sample or warmup count is outside its bound")
    api_key = os.environ.get("IMMICH_RS_BENCHMARK_API_KEY")
    if not api_key:
        raise BenchmarkError("disposable benchmark key is missing")
    fixture, expected, media_bytes = fixture_identity(
        arguments.fixture_manifest, arguments.source
    )
    metrics = load_module(REPOSITORY_ROOT / "scripts/benchmark-metrics.py")
    raw = []
    for index in range(arguments.warmups + arguments.samples):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        pair: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            workspace = arguments.workspace / f"pair-{index}-{tool}"
            workspace.mkdir(parents=True)
            if tool == "immich_rs":
                pair[tool] = run_rust(
                    arguments.immich_rs, arguments.endpoint, api_key, workspace,
                    metrics, expected, media_bytes,
                )
            else:
                pair[tool] = run_go(
                    arguments.oracle, arguments.endpoint, api_key, workspace,
                    metrics, expected, media_bytes,
                )
        if index >= arguments.warmups:
            pair["sample"] = index - arguments.warmups
            raw.append(pair)
    baseline = tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    return {
        "schema": "phase5-benchmark-report-v1",
        "manifest": {
            "source_revision": arguments.source_revision,
            "fixture": {
                "id": fixture["fixture_id"], "kind": "synthetic", "license": fixture["license"],
                "manifest_sha256": sha256(arguments.fixture_manifest),
                "original_assets": 4, "logical_media_bytes": media_bytes,
            },
            "tools": {
                "immich_rs": {"version": tool_version([str(arguments.immich_rs), "--version"]), "sha256": sha256(arguments.immich_rs)},
                "immich_go": {**baseline["oracle"], **baseline["artifacts"]["linux_x86_64"], "sha256": sha256(arguments.oracle)},
            },
            "environment": {
                "system": platform.system(), "kernel": platform.release(),
                "architecture": platform.machine(), "logical_cpus": os.cpu_count(),
                "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>",
            },
            "methodology": {
                "samples": arguments.samples, "warmups": arguments.warmups,
                "pairing": "same owner, originals, disposable server and warm cache",
                "order": "alternating within pairs", "concurrency": 1,
                "scope": "inventory plus original-byte archive; startup and verification excluded",
                "percentiles": "nearest-rank p95 over the raw samples",
            },
        },
        "raw_samples": raw,
        "aggregate": {"immich_rs": aggregate(raw, "immich_rs"), "immich_go": aggregate(raw, "immich_go")},
        "claims": ["Raw measurements only; no performance improvement is claimed."],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--fixture-manifest", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--immich-rs", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        report = run(arguments)
        arguments.output.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (BenchmarkError, OSError, UnicodeError, ValueError, KeyError) as error:
        print(f"Phase 5 benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
