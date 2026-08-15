#!/usr/bin/env python3
"""Validate committed Phase 3 paired benchmark evidence."""

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
FIXTURE_ROOT = REPOSITORY_ROOT / "tests/fixtures/v2/synthetic-google-takeout-complete"
BASELINE_PATH = REPOSITORY_ROOT / "tests/oracle/baseline.toml"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
METRICS = {
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds",
    "peak_rss_bytes", "peak_open_file_descriptors", "characters_read",
    "characters_written", "storage_bytes_read", "storage_bytes_written",
    "logical_media_bytes_read", "logical_media_bytes_written",
}
ARCHIVES = [
    {
        "bytes": 1451,
        "name": "takeout-001.zip",
        "sha256": "6dc4dd0ead12d05c9f868f6a0362bf67b59c10c35b4428350ae50f37e316d626",
    },
    {
        "bytes": 1126,
        "name": "takeout-002.zip",
        "sha256": "84c71866e80bb19c644ee987f6df0903fe64f7f4873d5a779abda17ebbf1154a",
    },
]
RUST_OPERATIONS = {
    "contained_oracle_mutations": 0, "http_requests": 0,
    "http_retries_observed": 0, "metadata_candidates": 5,
    "physical_assets_observed": 4, "source_assets_planned": 3,
}
ORACLE_OPERATIONS = {
    **RUST_OPERATIONS,
    "contained_oracle_mutations": 5,
    "http_requests": 17,
}


class EvidenceError(ValueError):
    """Phase 3 benchmark evidence is incomplete or inconsistent."""


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _numeric(value: object) -> bool:
    return not isinstance(value, bool) and isinstance(value, (int, float)) and value >= 0


def _aggregate(samples: list[dict[str, Any]], tool: str, field: str) -> dict[str, Any]:
    values = sorted(sample[tool][field] for sample in samples)
    p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
    return {
        "min": min(values), "median": statistics.median(values),
        "p95": values[p95_index], "max": max(values),
    }


def validate(path: Path) -> None:
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"{path}: cannot read report") from error
    if not isinstance(report, dict) or report.get("schema") != "phase3-benchmark-report-v1":
        raise EvidenceError(f"{path}: unsupported report schema")
    encoded = json.dumps(report, sort_keys=True)
    forbidden = ("/tmp/", "127.0.0.1", "accessToken", "api_key", "password")
    if any(value in encoded for value in forbidden):
        raise EvidenceError(f"{path}: report contains a sensitive runtime value")
    manifest = report.get("manifest")
    if not isinstance(manifest, dict) or COMMIT.fullmatch(str(manifest.get("source_revision"))) is None:
        raise EvidenceError(f"{path}: source revision is invalid")
    fixture = manifest.get("fixture")
    expected_fixture = {
        "id": "synthetic-google-takeout-complete",
        "manifest_sha256": _sha256(FIXTURE_ROOT / "manifest.json"),
        "expected_plan_sha256": _sha256(FIXTURE_ROOT / "expected-plan.json"),
        "logical_source_bytes": 1296,
        "logical_assets": 3,
        "physical_assets": 4,
        "sidecars": 5,
        "archive_view": "split",
        "archives": ARCHIVES,
    }
    if fixture != expected_fixture:
        raise EvidenceError(f"{path}: fixture or archive identity drift")
    baseline = tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    tools = manifest.get("tools")
    rust = tools.get("immich_rs") if isinstance(tools, dict) else None
    oracle = tools.get("immich_go") if isinstance(tools, dict) else None
    expected_oracle = baseline["artifacts"]["linux_x86_64"]["binary_sha256"]
    if (
        not isinstance(rust, dict)
        or not isinstance(rust.get("sha256"), str)
        or SHA256.fullmatch(rust["sha256"]) is None
        or not isinstance(oracle, dict)
        or oracle.get("version") != baseline["oracle"]["version"]
        or oracle.get("binary_sha256") != expected_oracle
        or oracle.get("sha256") != expected_oracle
    ):
        raise EvidenceError(f"{path}: benchmark tool identity drift")
    environment = manifest.get("environment")
    if not isinstance(environment, dict) or environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError(f"{path}: environment is not redacted")
    methodology = manifest.get("methodology")
    samples = report.get("raw_samples")
    if (
        not isinstance(methodology, dict)
        or methodology.get("samples") != 6
        or methodology.get("warmups") != 2
        or methodology.get("concurrency") != 1
        or methodology.get("immich_rs_buffer_bytes") != 65_536
        or methodology.get("percentiles") != "nearest-rank p95 over the raw samples"
        or not isinstance(samples, list)
        or len(samples) != 6
    ):
        raise EvidenceError(f"{path}: methodology or raw sample count drift")
    for index, sample in enumerate(samples):
        order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"
        if sample.get("sample") != index or sample.get("execution_order") != order:
            raise EvidenceError(f"{path}: paired sample order drift")
        for tool, operations in (
            ("immich_rs", RUST_OPERATIONS), ("immich_go", ORACLE_OPERATIONS)
        ):
            measured = sample.get(tool)
            if (
                not isinstance(measured, dict)
                or any(not _numeric(measured.get(field)) for field in METRICS)
                or measured.get("logical_media_bytes_read") != 1296
                or measured.get("logical_media_bytes_written") != 0
                or measured.get("operations") != operations
            ):
                raise EvidenceError(f"{path}: {tool} metrics or operations drift")
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
    paths = sorted(EVIDENCE_ROOT.glob("phase3-*.json"))
    try:
        if not paths:
            raise EvidenceError("Phase 3 benchmark evidence is missing")
        for path in paths:
            validate(path)
    except (EvidenceError, OSError, UnicodeError, KeyError, TypeError) as error:
        print(f"Phase 3 benchmark evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"Phase 3 benchmark evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
