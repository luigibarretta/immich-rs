#!/usr/bin/env python3
"""Run paired Phase 1 process benchmarks on one synthetic corpus per sample."""

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
import subprocess
import sys
from types import ModuleType
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
CASE_PATH = REPOSITORY_ROOT / "tests" / "oracle" / "cases" / "benchmark-v1.json"
EXPECTED_PLAN_PATH = REPOSITORY_ROOT / "tests" / "fixtures" / "v1" / "synthetic-benchmark" / "expected-plan.json"
METRIC_FIELDS = (
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


class PhaseBenchmarkError(RuntimeError):
    """A paired benchmark invariant failed."""


def _load_python(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise PhaseBenchmarkError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def _command_output(command: list[str]) -> str:
    try:
        result = subprocess.run(command, check=True, capture_output=True, text=True, timeout=15)
    except (OSError, subprocess.SubprocessError) as error:
        raise PhaseBenchmarkError(f"cannot inspect benchmark tool: {error}") from error
    return (result.stdout or result.stderr).strip()


def _oracle_log_paths(observation: dict[str, Any], marker: str) -> set[str]:
    paths: set[str] = set()
    logs = observation.get("process", {}).get("logs", [])
    needle = f"{marker} file=fixture:"
    for log in logs:
        for line in log.get("lines", []):
            if needle in line:
                paths.add(line.split(needle, 1)[1].split(" reason=", 1)[0])
    return paths


def _run_pair(
    index: int,
    oracle: Path,
    immich_rs: Path,
    expected_plan: dict[str, Any],
    runner: ModuleType,
    metrics_module: ModuleType,
) -> dict[str, Any]:
    captured: dict[str, Any] = {}
    order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"

    def run_immich_rs(source_root: Path, cwd: Path, environment: dict[str, str], timeout: int) -> None:
        command = [
            str(immich_rs),
            "plan",
            "folder",
            str(source_root),
            "--label",
            "synthetic-benchmark",
            "--buffer-bytes",
            "65536",
        ]
        result, measured = metrics_module.run_command(command, cwd, environment, timeout)
        if result.returncode != 0:
            raise PhaseBenchmarkError(f"immich-rs benchmark failed: {result.stderr}")
        try:
            plan = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise PhaseBenchmarkError(f"immich-rs returned invalid JSON: {error}") from error
        if plan != expected_plan:
            raise PhaseBenchmarkError("immich-rs benchmark plan differs from the golden plan")
        measured["operations"] = {
            "source_assets_planned": plan["summary"]["assets"],
            "http_requests": 0,
            "http_retries_observed": 0,
            "contained_oracle_mutations": 0,
        }
        measured["logical_media_bytes_read"] = plan["summary"]["bytes_read"]
        measured["logical_media_bytes_written"] = 0
        captured["immich_rs"] = measured

    def execute_oracle(
        command: list[str], cwd: Path, environment: dict[str, str], timeout: int
    ) -> subprocess.CompletedProcess[str]:
        source_root = Path(command[-1])
        if order == "immich-rs-first":
            run_immich_rs(source_root, cwd, environment, timeout)
        oracle_result, oracle_metrics = metrics_module.run_command(command, cwd, environment, timeout)
        captured["immich_go"] = oracle_metrics
        if order == "immich-go-first":
            run_immich_rs(source_root, cwd, environment, timeout)
        return oracle_result

    observation = runner.run_case(CASE_PATH, oracle, process_executor=execute_oracle)
    observable = observation["observable"]
    request_count = sum(request["count"] for request in observable["requests"])
    retry_count = sum(max(0, request["count"] - 1) for request in observable["requests"])
    captured["immich_go"]["operations"] = {
        "source_assets_planned": len(_oracle_log_paths(observation, "uploaded successfully")),
        "http_requests": request_count,
        "http_retries_observed": retry_count,
        "contained_oracle_mutations": len(observable["committed_mutations"]),
    }
    captured["immich_go"]["logical_media_bytes_read"] = expected_plan["summary"]["bytes_read"]
    captured["immich_go"]["logical_media_bytes_written"] = 0
    return {
        "sample": index,
        "execution_order": order,
        "immich_rs": captured["immich_rs"],
        "immich_go": captured["immich_go"],
    }


def _aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for field in METRIC_FIELDS:
        values = sorted(sample[tool][field] for sample in samples)
        p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {
            "min": min(values),
            "median": statistics.median(values),
            "p95": values[p95_index],
            "max": max(values),
        }
    return result


def run(samples: int, warmups: int, oracle: Path, immich_rs: Path) -> dict[str, Any]:
    if samples < 2 or samples > 30 or warmups < 0 or warmups > 4:
        raise PhaseBenchmarkError("samples must be 2..30 and warmups must be 0..4")
    if not oracle.is_file() or not immich_rs.is_file():
        raise PhaseBenchmarkError("benchmark binaries must already exist")
    runner = _load_python(REPOSITORY_ROOT / "scripts" / "run-oracle.py", "phase1_oracle_runner")
    metrics_module = _load_python(REPOSITORY_ROOT / "scripts" / "benchmark-metrics.py", "phase1_metrics")
    expected_plan = json.loads(EXPECTED_PLAN_PATH.read_text(encoding="utf-8"))
    raw: list[dict[str, Any]] = []
    for index in range(warmups + samples):
        pair = _run_pair(index, oracle, immich_rs, expected_plan, runner, metrics_module)
        if index >= warmups:
            pair["sample"] = index - warmups
            raw.append(pair)
    baseline = runner.load_baseline()
    source_revision = os.environ.get("SOURCE_REVISION") or os.environ.get("GITHUB_SHA")
    if not isinstance(source_revision, str) or re.fullmatch(r"[0-9a-f]{40,64}", source_revision) is None:
        raise PhaseBenchmarkError("SOURCE_REVISION or GITHUB_SHA must contain the exact source commit")
    return {
        "schema": "phase1-benchmark-report-v1",
        "manifest": {
            "source_revision": source_revision,
            "fixture": {
                "id": "synthetic-benchmark",
                "manifest_sha256": _sha256(
                    REPOSITORY_ROOT / "tests" / "fixtures" / "v1" / "synthetic-benchmark" / "manifest.json"
                ),
                "expected_plan_sha256": _sha256(EXPECTED_PLAN_PATH),
                "media_bytes": expected_plan["summary"]["bytes_read"],
                "assets": expected_plan["summary"]["assets"],
            },
            "tools": {
                "immich_rs": {"version": _command_output([str(immich_rs), "--version"]), "sha256": _sha256(immich_rs)},
                "immich_go": {**baseline, "sha256": _sha256(oracle)},
                "rustc": _command_output(["rustc", "--version"]),
            },
            "environment": {
                "system": platform.system(),
                "kernel": platform.release(),
                "architecture": platform.machine(),
                "logical_cpus": os.cpu_count(),
                "locale": "C.UTF-8",
                "timezone": "UTC",
                "hostname": "<REDACTED_HOST>",
            },
            "methodology": {
                "samples": samples,
                "warmups": warmups,
                "pairing": "same materialized corpus and environment within each sample",
                "order": "alternating to bound page-cache ordering bias",
                "scope": "child process only; fixture generation and mock startup excluded",
                "percentiles": "nearest-rank p95 over the raw samples",
                "concurrency": 1,
                "immich_rs_buffer_bytes": 65_536,
            },
        },
        "raw_samples": raw,
        "aggregate": {
            "immich_rs": _aggregate(raw, "immich_rs"),
            "immich_go": _aggregate(raw, "immich_go"),
        },
        "claims": ["Raw measurements only; no performance improvement is claimed."],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--oracle", type=Path, default=Path(os.environ.get("IMMICH_GO_ORACLE", "/usr/local/bin/immich-go")))
    parser.add_argument("--immich-rs", type=Path, default=REPOSITORY_ROOT / "target" / "release" / "immich-rs")
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        report = run(arguments.samples, arguments.warmups, arguments.oracle.resolve(), arguments.immich_rs.resolve())
    except (PhaseBenchmarkError, OSError, UnicodeError, ValueError) as error:
        print(f"Phase 1 benchmark failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
