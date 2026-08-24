#!/usr/bin/env python3
"""Benchmark paired Immich migrations on fresh two-server synthetic mocks."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import runpy
import statistics
import subprocess
import sys
import tempfile
import tomllib
from types import ModuleType
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "tests/oracle/baseline.toml"
FIXTURE = ROOT / "tests/oracle/server-fixtures/immich-migration-v1.json"
MOCK = runpy.run_path(str(ROOT / "tests/oracle/mock_immich_server.py"))
MIGRATION = runpy.run_path(str(ROOT / "tests/oracle/mock_migration.py"))
SOURCE_KEY = MOCK["SYNTHETIC_MIGRATION_SOURCE_KEY"]
DESTINATION_KEY = MOCK["SYNTHETIC_MIGRATION_DESTINATION_KEY"]
METRICS = (
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds", "peak_rss_bytes",
    "peak_open_file_descriptors", "characters_read", "characters_written",
    "storage_bytes_read", "storage_bytes_written",
)
SUM_METRICS = set(METRICS) - {"peak_rss_bytes", "peak_open_file_descriptors"}


class BenchmarkError(RuntimeError):
    """The paired migration benchmark violated its reproducibility contract."""


def load_metrics() -> ModuleType:
    path = ROOT / "scripts/benchmark-metrics.py"
    spec = importlib.util.spec_from_file_location("migration_metrics", path)
    if spec is None or spec.loader is None:
        raise BenchmarkError("cannot load process metrics")
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


def benchmark_scenario(key: str, *, source: bool) -> dict[str, Any]:
    selected = MIGRATION["scenario"](MOCK["default_scenario"], key, source=source)
    if source:
        selected["archive_assets"] = [
            {key: value for key, value in asset.items() if key != "live_photo_video_id"}
            for asset in selected["archive_assets"] if asset["visibility"] == "timeline"
        ]
    return selected


def environment(home: Path, *, rust: bool) -> dict[str, str]:
    home.mkdir()
    selected = {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "NO_COLOR": "1", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC",
    }
    if rust:
        selected["IMMICH_RS_SOURCE_API_KEY"] = SOURCE_KEY
        selected["IMMICH_RS_DESTINATION_API_KEY"] = DESTINATION_KEY
    return selected


def resource_arguments() -> list[str]:
    return [
        "--page-size", "1", "--max-assets", "2", "--max-albums", "1",
        "--max-album-memberships", "2", "--max-asset-bytes", "1024",
        "--max-total-bytes", "4096", "--concurrency", "1",
    ]


def combine(values: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        field: (
            sum(value[field] for value in values)
            if field in SUM_METRICS else max(value[field] for value in values)
        )
        for field in METRICS
    }


def validate_outcome(source: Any, destination: Any) -> dict[str, int]:
    source_state = source.state.snapshot()
    state = destination.state.snapshot()
    if source_state["committed_mutations"] or any(
        request["mutating"] for request in source_state["requests"]
    ):
        raise BenchmarkError("benchmark mutated its source")
    if (
        state["asset_count"] != 2 or state["metadata_count"] != 2
        or state["album_count"] != 1 or state["album_memberships"] != 2
    ):
        raise BenchmarkError("benchmark destination outcome drifted")
    return {
        "assets": 2, "metadata_updates": 2, "albums": 1,
        "album_memberships": 2, "logical_media_bytes": 62, "retries": 0,
    }


def run_rust(
    binary: Path, workspace: Path, source: Any, destination: Any, metrics: ModuleType,
) -> dict[str, Any]:
    home = workspace / "home"
    env = environment(home, rust=True)
    plan_path = workspace / "plan.json"
    checkpoint = workspace / "checkpoint.sqlite"
    common = ["--source-server", source.url, "--destination-server", destination.url]
    planned, plan_metrics = metrics.run_command(
        [str(binary), "plan", "migration", "immich", *common, *resource_arguments()],
        workspace, env, 60,
    )
    if planned.returncode != 0:
        raise BenchmarkError("immich-rs benchmark planning failed")
    plan_path.write_text(planned.stdout, encoding="utf-8")
    plan_path.chmod(0o600)
    applied, apply_metrics = metrics.run_command(
        [
            str(binary), "apply", "migration", "immich", "--plan", str(plan_path),
            "--checkpoint", str(checkpoint), *common, *resource_arguments(),
        ],
        workspace, env, 60,
    )
    if applied.returncode != 0:
        raise BenchmarkError("immich-rs benchmark apply failed")
    report = json.loads(applied.stdout)
    if report.get("created") != 2 or report.get("metadata_updated") != 2:
        raise BenchmarkError("immich-rs benchmark report drifted")
    return combine([plan_metrics, apply_metrics])


def run_go(
    binary: Path, workspace: Path, source: Any, destination: Any, metrics: ModuleType,
) -> dict[str, Any]:
    command = [
        str(binary), "upload", "from-immich", "--from-server", source.url,
        "--from-api-key", SOURCE_KEY, "--server", destination.url, "--api-key",
        DESTINATION_KEY, "--from-device-uuid", "synthetic-migration-source",
        "--device-uuid", "synthetic-migration-destination",
        "--from-pause-immich-jobs=false", "--pause-immich-jobs=false", "--no-ui",
        "--log-level", "ERROR", "--concurrent-tasks", "1", "--on-errors", "stop",
    ]
    completed, measured = metrics.run_command(
        command, workspace, environment(workspace / "home", rust=False), 60,
    )
    if completed.returncode != 0:
        raise BenchmarkError("immich-go benchmark migration failed")
    return measured


def sample_tool(
    tool: str, binary: Path, workspace: Path, metrics: ModuleType,
) -> dict[str, Any]:
    with (
        MOCK["running_mock"](benchmark_scenario(SOURCE_KEY, source=True)) as source,
        MOCK["running_mock"](benchmark_scenario(DESTINATION_KEY, source=False)) as destination,
    ):
        measured = (
            run_rust(binary, workspace, source, destination, metrics)
            if tool == "immich_rs"
            else run_go(binary, workspace, source, destination, metrics)
        )
        measured["operations"] = validate_outcome(source, destination)
    return measured


def aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result = {}
    for field in METRICS:
        values = sorted(sample[tool][field] for sample in samples)
        p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {
            "min": min(values), "median": statistics.median(values),
            "p95": values[p95_index], "max": max(values),
        }
    return result


def tool_version(binary: Path) -> str:
    completed = subprocess.run(
        [str(binary), "--version"], check=True, capture_output=True,
        text=True, timeout=15, env={"LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "")},
    )
    return (completed.stdout or completed.stderr).strip()


def run(options: argparse.Namespace) -> dict[str, Any]:
    if not 2 <= options.samples <= 10 or not 0 <= options.warmups <= 2:
        raise BenchmarkError("sample or warmup count is outside its bound")
    if len(options.source_revision) != 40 or any(
        character not in "0123456789abcdef" for character in options.source_revision
    ):
        raise BenchmarkError("source revision must be an exact lowercase SHA")
    metrics = load_metrics()
    raw = []
    for index in range(options.samples + options.warmups):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        pair: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            workspace = options.workspace / f"pair-{index}-{tool}"
            workspace.mkdir(parents=True)
            binary = options.immich_rs if tool == "immich_rs" else options.oracle
            pair[tool] = sample_tool(tool, binary, workspace, metrics)
        if index >= options.warmups:
            pair["sample"] = index - options.warmups
            raw.append(pair)
    aggregate_values = {tool: aggregate(raw, tool) for tool in ("immich_rs", "immich_go")}
    rust_wall = aggregate_values["immich_rs"]["wall_time_seconds"]
    go_wall = aggregate_values["immich_go"]["wall_time_seconds"]
    improvement = 100 * (go_wall["median"] - rust_wall["median"]) / go_wall["median"]
    claim = "Raw measurements only; no performance improvement is claimed."
    if improvement >= 10 and rust_wall["max"] < go_wall["min"]:
        claim = (
            "On this exact 62-byte two-asset synthetic two-server migration, "
            f"immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; "
            "this is not a large-library, real-server, WAN or production claim."
        )
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    return {
        "schema": "phase11-mock-benchmark-report-v1",
        "manifest": {
            "source_revision": options.source_revision,
            "fixture": {
                "id": "synthetic-immich-migration-standalone", "kind": "synthetic",
                "license": "CC0-1.0", "manifest_sha256": sha256(FIXTURE),
                "assets": 2, "logical_media_bytes": 62,
            },
            "tools": {
                "immich_rs": {"version": tool_version(options.immich_rs), "sha256": sha256(options.immich_rs)},
                "immich_go": {**baseline["oracle"], **baseline["artifacts"]["linux_x86_64"], "sha256": sha256(options.oracle)},
            },
            "environment": {
                "system": platform.system(), "kernel": platform.release(),
                "architecture": platform.machine(), "logical_cpus": os.cpu_count(),
                "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>",
            },
            "methodology": {
                "samples": options.samples, "warmups": options.warmups,
                "pairing": "fresh isolated source and destination mock per tool",
                "order": "alternating within pairs", "concurrency": 1,
                "scope": "complete inventory, planning and migration; mock startup excluded",
                "percentiles": "nearest-rank p95 over retained samples",
            },
        },
        "raw_samples": raw, "aggregate": aggregate_values, "claims": [claim],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--immich-rs", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    try:
        options.workspace = options.workspace.resolve()
        options.immich_rs = options.immich_rs.resolve()
        options.oracle = options.oracle.resolve()
        options.output = options.output.resolve()
        report = run(options)
        options.output.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (BenchmarkError, OSError, UnicodeError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"migration benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
