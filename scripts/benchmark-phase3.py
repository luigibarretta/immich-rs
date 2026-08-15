#!/usr/bin/env python3
"""Run paired Phase 3 read-only benchmarks on one synthetic split Takeout."""

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
CASE_PATH = REPOSITORY_ROOT / "tests/oracle/cases/google-takeout-complete-v2.json"
FIXTURE_ROOT = REPOSITORY_ROOT / "tests/fixtures/v2/synthetic-google-takeout-complete"
EXPECTED_PLAN_PATH = FIXTURE_ROOT / "expected-plan.json"
METRIC_FIELDS = (
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds",
    "peak_rss_bytes", "peak_open_file_descriptors", "characters_read",
    "characters_written", "storage_bytes_read", "storage_bytes_written",
    "logical_media_bytes_read", "logical_media_bytes_written",
)


class Phase3BenchmarkError(RuntimeError):
    """A paired Phase 3 benchmark invariant failed."""


def _load_python(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise Phase3BenchmarkError(f"cannot load {path}")
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
        result = subprocess.run(
            command, check=True, capture_output=True, text=True, timeout=15
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise Phase3BenchmarkError("cannot inspect benchmark tool") from error
    return (result.stdout or result.stderr).strip()


def _oracle_paths(observation: dict[str, Any], marker: str) -> set[str]:
    paths = set()
    pattern = re.compile(
        rf"{re.escape(marker)} file=takeout-\d+:(.+?)"
        r"(?:\s+(?:type|title|date|reason)=.*)?$"
    )
    for log in observation.get("process", {}).get("logs", []):
        for line in log.get("lines", []):
            if match := pattern.search(line):
                paths.add(match.group(1))
    return paths


def _archive_identity(paths: list[Path]) -> list[dict[str, Any]]:
    return [
        {"name": path.name, "bytes": path.stat().st_size, "sha256": _sha256(path)}
        for path in paths
    ]


def _run_pair(
    index: int,
    oracle: Path,
    immich_rs: Path,
    expected_plan: dict[str, Any],
    runner: ModuleType,
    metrics: ModuleType,
    archive_identity: list[dict[str, Any]] | None,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    captured: dict[str, Any] = {}
    order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"

    def run_rust(
        archives: list[Path], cwd: Path, environment: dict[str, str], timeout: int
    ) -> None:
        command = [
            str(immich_rs), "plan", "google-takeout", "--label",
            "synthetic-google-takeout-complete", "--buffer-bytes", "65536",
            *(str(path) for path in archives),
        ]
        result, measured = metrics.run_command(command, cwd, environment, timeout)
        if result.returncode != 0:
            raise Phase3BenchmarkError("immich-rs benchmark process failed")
        try:
            plan = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise Phase3BenchmarkError("immich-rs returned invalid JSON") from error
        if plan != expected_plan:
            raise Phase3BenchmarkError("immich-rs benchmark plan differs from its golden")
        measured["operations"] = {
            "physical_assets_observed": 4,
            "source_assets_planned": plan["summary"]["assets"],
            "metadata_candidates": plan["summary"]["sidecars"],
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
        archives = sorted(Path(value) for value in command if value.endswith(".zip"))
        if len(archives) != 2:
            raise Phase3BenchmarkError("oracle benchmark did not receive two ZIP parts")
        identity = _archive_identity(archives)
        if archive_identity is not None and identity != archive_identity:
            raise Phase3BenchmarkError("materialized archive identity drifted between samples")
        captured["archives"] = identity
        if order == "immich-rs-first":
            run_rust(archives, cwd, environment, timeout)
        oracle_result, measured = metrics.run_command(command, cwd, environment, timeout)
        captured["immich_go"] = measured
        if order == "immich-go-first":
            run_rust(archives, cwd, environment, timeout)
        return oracle_result

    observation = runner.run_case(CASE_PATH, oracle, process_executor=execute_oracle)
    observable = observation["observable"]
    requests = sum(request["count"] for request in observable["requests"])
    retries = sum(max(0, request["count"] - 1) for request in observable["requests"])
    physical = len(_oracle_paths(observation, "discovered image"))
    duplicates = len(_oracle_paths(observation, "discarded local duplicate"))
    captured["immich_go"]["operations"] = {
        "physical_assets_observed": physical,
        "source_assets_planned": physical - duplicates,
        "metadata_candidates": len(_oracle_paths(observation, "discovered sidecar")),
        "http_requests": requests,
        "http_retries_observed": retries,
        "contained_oracle_mutations": len(observable["committed_mutations"]),
    }
    captured["immich_go"]["logical_media_bytes_read"] = expected_plan["summary"]["bytes_read"]
    captured["immich_go"]["logical_media_bytes_written"] = 0
    sample = {
        "sample": index,
        "execution_order": order,
        "immich_rs": captured["immich_rs"],
        "immich_go": captured["immich_go"],
    }
    return sample, captured["archives"]


def _aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result = {}
    for field in METRIC_FIELDS:
        values = sorted(sample[tool][field] for sample in samples)
        p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {
            "min": min(values), "median": statistics.median(values),
            "p95": values[p95_index], "max": max(values),
        }
    return result


def run(samples: int, warmups: int, oracle: Path, immich_rs: Path) -> dict[str, Any]:
    if not 2 <= samples <= 30 or not 0 <= warmups <= 4:
        raise Phase3BenchmarkError("samples must be 2..30 and warmups must be 0..4")
    if not oracle.is_file() or not immich_rs.is_file():
        raise Phase3BenchmarkError("benchmark binaries must already exist")
    revision = os.environ.get("SOURCE_REVISION") or os.environ.get("GITHUB_SHA")
    if not isinstance(revision, str) or re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise Phase3BenchmarkError("source revision must contain the exact commit")
    runner = _load_python(REPOSITORY_ROOT / "scripts/run-oracle.py", "phase3_runner")
    metrics = _load_python(REPOSITORY_ROOT / "scripts/benchmark-metrics.py", "phase3_metrics")
    expected_plan = json.loads(EXPECTED_PLAN_PATH.read_text(encoding="utf-8"))
    raw = []
    archives = None
    for index in range(warmups + samples):
        pair, archives = _run_pair(
            index, oracle, immich_rs, expected_plan, runner, metrics, archives
        )
        if index >= warmups:
            pair["sample"] = index - warmups
            raw.append(pair)
    baseline = runner.load_baseline()
    return {
        "schema": "phase3-benchmark-report-v1",
        "manifest": {
            "source_revision": revision,
            "fixture": {
                "id": "synthetic-google-takeout-complete",
                "manifest_sha256": _sha256(FIXTURE_ROOT / "manifest.json"),
                "expected_plan_sha256": _sha256(EXPECTED_PLAN_PATH),
                "logical_source_bytes": expected_plan["summary"]["bytes_read"],
                "logical_assets": expected_plan["summary"]["assets"],
                "physical_assets": 4,
                "sidecars": 5,
                "archive_view": "split",
                "archives": archives,
            },
            "tools": {
                "immich_rs": {
                    "version": _command_output([str(immich_rs), "--version"]),
                    "sha256": _sha256(immich_rs),
                },
                "immich_go": {**baseline, "sha256": _sha256(oracle)},
                "rustc": _command_output(["rustc", "--version"]),
            },
            "environment": {
                "system": platform.system(), "kernel": platform.release(),
                "architecture": platform.machine(), "logical_cpus": os.cpu_count(),
                "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>",
            },
            "methodology": {
                "samples": samples, "warmups": warmups,
                "pairing": "same materialized split ZIP corpus and mock environment within each sample",
                "order": "alternating to bound page-cache ordering bias",
                "scope": "child process only; fixture generation and mock startup excluded",
                "percentiles": "nearest-rank p95 over the raw samples",
                "concurrency": 1, "immich_rs_buffer_bytes": 65_536,
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
    parser.add_argument("--oracle", type=Path, default=Path("/usr/local/bin/immich-go"))
    parser.add_argument("--immich-rs", type=Path, default=REPOSITORY_ROOT / "target/release/immich-rs")
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        report = run(
            arguments.samples, arguments.warmups,
            arguments.oracle.resolve(), arguments.immich_rs.resolve(),
        )
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    except (OSError, UnicodeError, ValueError, Phase3BenchmarkError) as error:
        print(f"Phase 3 benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
