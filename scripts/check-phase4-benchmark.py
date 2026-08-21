#!/usr/bin/env python3
"""Validate committed Phase 4 paired benchmark evidence."""

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
EVIDENCE_ROOT = REPOSITORY_ROOT / "benchmarks/evidence"
FIXTURE_ROOT = REPOSITORY_ROOT / "tests/fixtures/v3/synthetic-apple-photos"
BASELINE_PATH = REPOSITORY_ROOT / "tests/oracle/baseline.toml"
METRICS = {
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds",
    "peak_rss_bytes", "peak_open_file_descriptors", "characters_read",
    "characters_written", "storage_bytes_read", "storage_bytes_written",
    "logical_media_bytes_read", "logical_media_bytes_written",
}
RUST_OPERATIONS = {
    "contained_oracle_mutations": 0,
    "http_requests": 0,
    "http_retries_observed": 0,
    "metadata_candidates": 1,
    "source_assets_planned": 5,
}
ORACLE_OPERATIONS = {
    "contained_oracle_mutations": 5,
    "http_requests": 17,
    "http_retries_observed": 0,
    "metadata_candidates": 1,
    "source_assets_planned": 5,
}


class EvidenceError(ValueError):
    """Committed Phase 4 benchmark evidence is incomplete or inconsistent."""


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
    if report.get("schema") != "phase4-benchmark-report-v1":
        raise EvidenceError(f"{path}: unsupported Phase 4 report schema")
    manifest = report.get("manifest")
    if not isinstance(manifest, dict):
        raise EvidenceError(f"{path}: manifest is missing")
    revision = manifest.get("source_revision")
    if not isinstance(revision, str) or re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise EvidenceError(f"{path}: source revision is invalid")
    fixture = manifest.get("fixture")
    if not isinstance(fixture, dict):
        raise EvidenceError(f"{path}: fixture identity is missing")
    expected_fixture = {
        "id": "synthetic-apple-photos",
        "manifest_sha256": _sha256(FIXTURE_ROOT / "manifest.json"),
        "expected_plan_sha256": _sha256(FIXTURE_ROOT / "expected-plan.json"),
        "logical_source_bytes": 426,
        "logical_assets": 5,
        "sidecars": 1,
        "archive_view": "icloud-split",
    }
    if any(fixture.get(key) != value for key, value in expected_fixture.items()):
        raise EvidenceError(f"{path}: fixture identity drift")
    archives = fixture.get("archives")
    if not isinstance(archives, list) or len(archives) != 2 or any(
        not isinstance(item, dict)
        or set(item) != {"name", "bytes", "sha256"}
        or not isinstance(item["bytes"], int)
        or re.fullmatch(r"[0-9a-f]{64}", item.get("sha256", "")) is None
        for item in archives
    ):
        raise EvidenceError(f"{path}: archive identities are incomplete")
    baseline = tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    expected_oracle = baseline["artifacts"]["linux_x86_64"]["binary_sha256"]
    tools = manifest.get("tools")
    oracle = tools.get("immich_go") if isinstance(tools, dict) else None
    if (
        not isinstance(oracle, dict)
        or oracle.get("version") != baseline["oracle"]["version"]
        or oracle.get("binary_sha256") != expected_oracle
        or oracle.get("sha256") != expected_oracle
    ):
        raise EvidenceError(f"{path}: oracle identity drift")
    environment = manifest.get("environment")
    if not isinstance(environment, dict) or environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError(f"{path}: environment is not redacted")
    methodology = manifest.get("methodology")
    samples = report.get("raw_samples")
    if not isinstance(methodology, dict) or not isinstance(samples, list):
        raise EvidenceError(f"{path}: methodology or samples are missing")
    if methodology.get("samples") != 6 or methodology.get("warmups") != 2 or len(samples) != 6:
        raise EvidenceError(f"{path}: sample count mismatch")
    if methodology.get("percentiles") != "nearest-rank p95 over the raw samples":
        raise EvidenceError(f"{path}: percentile method is missing")
    for index, sample in enumerate(samples):
        expected_order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"
        if sample.get("sample") != index or sample.get("execution_order") != expected_order:
            raise EvidenceError(f"{path}: pair order drift")
        for tool, operations in (
            ("immich_rs", RUST_OPERATIONS),
            ("immich_go", ORACLE_OPERATIONS),
        ):
            measured = sample.get(tool)
            if not isinstance(measured, dict) or any(
                not _numeric(measured.get(field)) for field in METRICS
            ):
                raise EvidenceError(f"{path}: {tool} metrics are incomplete")
            if measured.get("operations") != operations:
                raise EvidenceError(f"{path}: {tool} operations drift")
    aggregate = report.get("aggregate")
    if not isinstance(aggregate, dict):
        raise EvidenceError(f"{path}: aggregate is missing")
    for tool in ("immich_rs", "immich_go"):
        for field in METRICS:
            if aggregate.get(tool, {}).get(field) != _aggregate(samples, tool, field):
                raise EvidenceError(f"{path}: aggregate drift for {tool}.{field}")
    if report.get("claims") != ["Raw measurements only; no performance improvement is claimed."]:
        raise EvidenceError(f"{path}: unsupported performance claim")


def main() -> int:
    paths = sorted(EVIDENCE_ROOT.glob("phase4-*.json")) if EVIDENCE_ROOT.exists() else []
    try:
        if not paths:
            raise EvidenceError("Phase 4 benchmark evidence is missing")
        for path in paths:
            validate(path)
    except (EvidenceError, OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        print(f"Phase 4 benchmark evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"Phase 4 benchmark evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
