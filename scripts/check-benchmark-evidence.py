#!/usr/bin/env python3
"""Validate committed Phase 1 raw benchmark evidence and input digests."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import statistics
import sys
import tomllib
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = REPOSITORY_ROOT / "benchmarks" / "evidence"
FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "fixtures" / "v1" / "synthetic-benchmark"
BASELINE_PATH = REPOSITORY_ROOT / "tests" / "oracle" / "baseline.toml"
METRICS = {
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
}


class EvidenceError(ValueError):
    """Committed benchmark evidence is incomplete or inconsistent."""


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError(f"{path} must contain a JSON object")
    return value


def _numeric(value: object) -> bool:
    return not isinstance(value, bool) and isinstance(value, (int, float)) and value >= 0


def _aggregate(samples: list[dict[str, Any]], tool: str, field: str) -> dict[str, Any]:
    values = sorted(sample[tool][field] for sample in samples)
    p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
    return {
        "min": min(values),
        "median": statistics.median(values),
        "p95": values[p95_index],
        "max": max(values),
    }


def validate(path: Path) -> None:
    report = _object(path)
    if report.get("schema") != "phase1-benchmark-report-v1":
        raise EvidenceError(f"{path}: unsupported report schema")
    manifest = report.get("manifest")
    if not isinstance(manifest, dict):
        raise EvidenceError(f"{path}: manifest is missing")
    revision = manifest.get("source_revision")
    if not isinstance(revision, str) or re.fullmatch(r"[0-9a-f]{40,64}", revision) is None:
        raise EvidenceError(f"{path}: source revision is invalid")
    fixture = manifest.get("fixture")
    if not isinstance(fixture, dict):
        raise EvidenceError(f"{path}: fixture manifest is missing")
    expected_digests = {
        "manifest_sha256": _sha256(FIXTURE_ROOT / "manifest.json"),
        "expected_plan_sha256": _sha256(FIXTURE_ROOT / "expected-plan.json"),
    }
    if any(fixture.get(key) != digest for key, digest in expected_digests.items()):
        raise EvidenceError(f"{path}: fixture digest drift")
    media_bytes = fixture.get("media_bytes")
    assets = fixture.get("assets")
    if media_bytes != 67_108_864 or assets != 8:
        raise EvidenceError(f"{path}: unexpected benchmark corpus dimensions")
    environment = manifest.get("environment")
    if not isinstance(environment, dict) or environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError(f"{path}: environment hostname must be redacted")
    try:
        baseline = tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))
        expected_oracle = baseline["artifacts"]["linux_x86_64"]["binary_sha256"]
        expected_version = baseline["oracle"]["version"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise EvidenceError(f"{path}: cannot read oracle baseline: {error}") from error
    tools = manifest.get("tools")
    oracle = tools.get("immich_go") if isinstance(tools, dict) else None
    if (
        not isinstance(oracle, dict)
        or oracle.get("version") != expected_version
        or oracle.get("binary_sha256") != expected_oracle
        or oracle.get("sha256") != expected_oracle
    ):
        raise EvidenceError(f"{path}: oracle identity drift")
    methodology = manifest.get("methodology")
    samples = report.get("raw_samples")
    if not isinstance(methodology, dict) or not isinstance(samples, list):
        raise EvidenceError(f"{path}: methodology or raw samples are missing")
    if methodology.get("samples") != len(samples) or len(samples) < 2:
        raise EvidenceError(f"{path}: sample count mismatch")
    if methodology.get("percentiles") != "nearest-rank p95 over the raw samples":
        raise EvidenceError(f"{path}: percentile method is missing")
    for index, sample in enumerate(samples):
        expected_order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"
        if sample.get("sample") != index or sample.get("execution_order") != expected_order:
            raise EvidenceError(f"{path}: paired sample order is not reproducible")
        for tool in ("immich_rs", "immich_go"):
            measured = sample.get(tool)
            if not isinstance(measured, dict) or any(not _numeric(measured.get(field)) for field in METRICS):
                raise EvidenceError(f"{path}: {tool} sample metrics are incomplete")
            operations = measured.get("operations")
            if not isinstance(operations, dict) or operations.get("source_assets_planned") != assets:
                raise EvidenceError(f"{path}: {tool} operation counts are incomplete")
            if operations.get("http_retries_observed") != 0:
                raise EvidenceError(f"{path}: unexpected retry in no-fault benchmark")
        if sample["immich_rs"]["operations"] != {
            "contained_oracle_mutations": 0,
            "http_requests": 0,
            "http_retries_observed": 0,
            "source_assets_planned": 8,
        }:
            raise EvidenceError(f"{path}: immich-rs acquired an unexpected server capability")
        if sample["immich_go"]["operations"] != {
            "contained_oracle_mutations": 5,
            "http_requests": 17,
            "http_retries_observed": 0,
            "source_assets_planned": 8,
        }:
            raise EvidenceError(f"{path}: oracle operations drifted")
    aggregate = report.get("aggregate")
    if not isinstance(aggregate, dict):
        raise EvidenceError(f"{path}: aggregate is missing")
    for tool in ("immich_rs", "immich_go"):
        for field in METRICS:
            if aggregate.get(tool, {}).get(field) != _aggregate(samples, tool, field):
                raise EvidenceError(f"{path}: aggregate drift for {tool}.{field}")
    if aggregate["immich_rs"]["peak_rss_bytes"]["max"] >= media_bytes // assets:
        raise EvidenceError(f"{path}: immich-rs RSS does not prove sub-file memory use")
    if report.get("claims") != ["Raw measurements only; no performance improvement is claimed."]:
        raise EvidenceError(f"{path}: unsupported performance claim")


def main() -> int:
    paths = sorted(EVIDENCE_ROOT.glob("phase1-*.json")) if EVIDENCE_ROOT.exists() else []
    try:
        if not paths:
            raise EvidenceError("Phase 1 benchmark evidence is missing")
        for path in paths:
            validate(path)
    except (EvidenceError, OSError, UnicodeError) as error:
        print(f"benchmark evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"benchmark evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
