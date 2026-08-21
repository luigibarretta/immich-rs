#!/usr/bin/env python3
"""Validate Phase 5 disposable and paired archive evidence."""

from __future__ import annotations

import json
from pathlib import Path
import re
import statistics
import sys
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
BENCHMARK = ROOT / "benchmarks/evidence/phase5-2026-08-22.json"
DISPOSABLE = ROOT / "docs/evidence/phase5-disposable-archive-2026-08-22.json"
BASELINE = ROOT / "tests/oracle/baseline.toml"
SHA256 = re.compile(r"[0-9a-f]{64}")
COMMIT = re.compile(r"[0-9a-f]{40}")
IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}
METRICS = {
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds",
    "peak_rss_bytes", "peak_open_file_descriptors", "characters_read",
    "characters_written", "storage_bytes_read", "storage_bytes_written",
    "logical_media_bytes_read", "logical_media_bytes_written",
}
OPERATIONS = {"media_bytes": 587015, "original_assets": 4, "retries": 0}


class EvidenceError(ValueError):
    """Committed evidence is incomplete, private, or internally inconsistent."""


def load(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError(f"{path} must contain an object")
    return value


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def numeric(value: object, *, positive: bool = False) -> bool:
    return (
        not isinstance(value, bool)
        and isinstance(value, (int, float))
        and (value > 0 if positive else value >= 0)
    )


def digest(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise EvidenceError(f"{label} is not a SHA-256 digest")
    return value


def aggregate(samples: list[dict[str, Any]], tool: str, field: str) -> dict[str, Any]:
    values = sorted(sample[tool][field] for sample in samples)
    index = max(0, (95 * len(values) + 99) // 100 - 1)
    return {
        "min": min(values),
        "median": statistics.median(values),
        "p95": values[index],
        "max": max(values),
    }


def validate_disposable(report: dict[str, Any]) -> tuple[str, str]:
    if report.get("schema") != "phase5-disposable-archive-v1":
        raise EvidenceError("unsupported Phase 5 disposable schema")
    revision = report.get("commit_sha")
    if not isinstance(revision, str) or COMMIT.fullmatch(revision) is None:
        raise EvidenceError("Phase 5 implementation revision is invalid")
    environment = object_value(report.get("environment"), "disposable environment")
    binary = digest(environment.get("binary_sha256"), "disposable binary")
    if environment.get("os") != "Linux" or environment.get("architecture") != "x86_64":
        raise EvidenceError("disposable platform drift")
    if report.get("images") != IMAGES:
        raise EvidenceError("disposable image digest drift")
    fixture = object_value(report.get("fixture"), "disposable fixture")
    if fixture.get("kind") != "synthetic" or fixture.get("license") != "CC0-1.0":
        raise EvidenceError("disposable fixture is not synthetic CC0")
    digest(fixture.get("manifest_sha256"), "disposable fixture manifest")
    digest(report.get("archive_manifest_sha256"), "archive manifest")
    if report.get("server_version") != {"major": 3, "minor": 1, "patch": 0, "prerelease": None}:
        raise EvidenceError("disposable server version drift")
    methodology = object_value(report.get("methodology"), "disposable methodology")
    if methodology.get("commands") != [
        "seed synthetic folder upload", "plan archive immich", "apply archive",
        "apply archive (verified resume)",
    ] or "loopback-only" not in methodology.get("network", ""):
        raise EvidenceError("disposable isolation methodology drift")
    reports = object_value(report.get("reports"), "archive reports")
    first = object_value(reports.get("first"), "first archive report")
    second = object_value(reports.get("second"), "second archive report")
    manifest = digest(first.get("manifest_sha256"), "apply manifest")
    expected_first = {
        "schema_version": 1, "manifest_sha256": manifest, "downloaded": 4,
        "already_complete": 0, "bytes_written": 587015, "retries": 0,
    }
    expected_second = {
        "schema_version": 1, "manifest_sha256": manifest, "downloaded": 0,
        "already_complete": 4, "bytes_written": 0, "retries": 0,
    }
    if first != expected_first or second != expected_second or report.get("archive_assets") != 4:
        raise EvidenceError("archive apply or idempotent resume counters drift")
    if report.get("cleanup") != {
        "verified": True, "labelled_containers": 0,
        "labelled_volumes": 0, "labelled_networks": 0,
    }:
        raise EvidenceError("disposable cleanup was not proven")
    return revision, binary


def validate_benchmark(report: dict[str, Any], revision: str, binary: str) -> None:
    if report.get("schema") != "phase5-benchmark-report-v1":
        raise EvidenceError("unsupported Phase 5 benchmark schema")
    manifest = object_value(report.get("manifest"), "benchmark manifest")
    if manifest.get("source_revision") != revision:
        raise EvidenceError("benchmark and disposable revisions differ")
    environment = object_value(manifest.get("environment"), "benchmark environment")
    if environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError("benchmark hostname is not redacted")
    fixture = object_value(manifest.get("fixture"), "benchmark fixture")
    expected_fixture = {
        "id": "synthetic-phase2-standalone-matrix", "kind": "synthetic",
        "license": "CC0-1.0", "logical_media_bytes": 587015,
        "original_assets": 4,
    }
    if any(fixture.get(key) != value for key, value in expected_fixture.items()):
        raise EvidenceError("benchmark fixture identity drift")
    digest(fixture.get("manifest_sha256"), "benchmark fixture manifest")
    methodology = object_value(manifest.get("methodology"), "benchmark methodology")
    expected_method = {
        "concurrency": 1, "order": "alternating within pairs",
        "pairing": "same owner, originals, disposable server and warm cache",
        "percentiles": "nearest-rank p95 over the raw samples", "samples": 6,
        "scope": "inventory plus original-byte archive; startup and verification excluded",
        "warmups": 2,
    }
    if methodology != expected_method:
        raise EvidenceError("benchmark methodology drift")
    tools = object_value(manifest.get("tools"), "benchmark tools")
    rust = object_value(tools.get("immich_rs"), "immich-rs identity")
    oracle = object_value(tools.get("immich_go"), "immich-go identity")
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    expected_oracle = baseline["artifacts"]["linux_x86_64"]["binary_sha256"]
    if rust.get("sha256") != binary or oracle.get("version") != baseline["oracle"]["version"]:
        raise EvidenceError("benchmark tool identity drift")
    if oracle.get("binary_sha256") != expected_oracle or oracle.get("sha256") != expected_oracle:
        raise EvidenceError("oracle digest drift")
    samples = report.get("raw_samples")
    if not isinstance(samples, list) or len(samples) != 6:
        raise EvidenceError("benchmark retained sample count drift")
    for index, sample in enumerate(samples):
        if not isinstance(sample, dict) or sample.get("sample") != index:
            raise EvidenceError("benchmark sample index drift")
        expected_order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"
        if sample.get("execution_order") != expected_order:
            raise EvidenceError("benchmark execution order drift")
        for tool in ("immich_rs", "immich_go"):
            measured = object_value(sample.get(tool), f"sample {index} {tool}")
            if measured.get("operations") != OPERATIONS or any(
                not numeric(measured.get(field)) for field in METRICS
            ):
                raise EvidenceError(f"sample {index} {tool} metrics drift")
    aggregates = object_value(report.get("aggregate"), "benchmark aggregates")
    for tool in ("immich_rs", "immich_go"):
        for field in METRICS:
            if aggregates.get(tool, {}).get(field) != aggregate(samples, tool, field):
                raise EvidenceError(f"aggregate drift for {tool}.{field}")
    rust_wall = aggregates["immich_rs"]["wall_time_seconds"]
    go_wall = aggregates["immich_go"]["wall_time_seconds"]
    improvement = 100 * (go_wall["median"] - rust_wall["median"]) / go_wall["median"]
    claim = (
        f"On this exact 587,015-byte four-asset synthetic disposable archive, "
        f"immich-rs median wall time was {improvement:.1f}% lower than immich-go "
        "v0.32.0; this is not a large-library or production claim."
    )
    if improvement < 10 or rust_wall["max"] >= go_wall["min"] or report.get("claims") != [claim]:
        raise EvidenceError("performance claim is unsupported by retained samples")


def main() -> int:
    try:
        disposable = load(DISPOSABLE)
        benchmark = load(BENCHMARK)
        revision, binary = validate_disposable(disposable)
        validate_benchmark(benchmark, revision, binary)
    except (EvidenceError, OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        print(f"Phase 5 evidence check failed: {error}", file=sys.stderr)
        return 1
    print("Phase 5 evidence passed: disposable and paired benchmark")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
