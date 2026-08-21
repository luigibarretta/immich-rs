#!/usr/bin/env python3
"""Validate large synthetic soak and authorized private shadow evidence."""

from __future__ import annotations

import json
from pathlib import Path
import re
import statistics
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
SOAK = ROOT / "docs/evidence/phase6-synthetic-soak-2026-08-22.json"
PRIVATE = ROOT / "docs/evidence/phase6-private-takeout-shadow-2026-08-22.json"
SHA256 = re.compile(r"[0-9a-f]{64}")
COMMIT = re.compile(r"[0-9a-f]{40}")
SAMPLE_METRICS = {
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds",
    "peak_rss_bytes", "peak_open_file_descriptors", "characters_read",
    "characters_written", "storage_bytes_read", "storage_bytes_written",
}


class EvidenceError(ValueError):
    """Committed Phase 6 scale evidence is incomplete or inconsistent."""


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


def validate_environment(value: object, label: str) -> tuple[str, str]:
    environment = object_value(value, label)
    revision = environment.get("source_revision")
    if not isinstance(revision, str) or COMMIT.fullmatch(revision) is None:
        raise EvidenceError(f"{label} source revision is invalid")
    binary = digest(environment.get("binary_sha256"), f"{label} binary")
    if (
        environment.get("system") != "Linux"
        or environment.get("architecture") != "x86_64"
        or environment.get("hostname") != "<REDACTED_HOST>"
        or not numeric(environment.get("logical_cpus"), positive=True)
    ):
        raise EvidenceError(f"{label} is incomplete or not redacted")
    return revision, binary


def validate_private(report: dict[str, Any]) -> str:
    if report.get("schema") != "authorized-takeout-shadow-v1":
        raise EvidenceError("unsupported private shadow schema")
    if report.get("authorization") != "explicit user-provided private read-only corpus":
        raise EvidenceError("private shadow authorization is missing")
    if report.get("privacy") != "aggregate counters only; no paths, names, metadata or content digests":
        raise EvidenceError("private shadow privacy contract drift")
    if report.get("cleanup_verified") is not True or report.get("deterministic_retained_runs") != 3:
        raise EvidenceError("private shadow determinism or cleanup was not proven")
    _, binary = validate_environment(report.get("environment"), "private environment")
    expected_fixture = {
        "archive_bytes": 455403635, "archive_entries": 1778, "media_candidates": 889,
    }
    expected_plan = {"assets": 889, "bytes_read": 455403635, "sidecars": 889}
    if report.get("fixture") != expected_fixture or report.get("plan_summary") != expected_plan:
        raise EvidenceError("private shadow aggregate counters drift")
    metrics = object_value(report.get("metrics"), "private metrics")
    wall = metrics.get("wall_time_seconds")
    if not isinstance(wall, list) or len(wall) != 3 or any(not numeric(item, positive=True) for item in wall):
        raise EvidenceError("private shadow samples are invalid")
    if metrics.get("median_wall_time_seconds") != statistics.median(wall):
        raise EvidenceError("private shadow median drift")
    if not numeric(metrics.get("peak_rss_bytes"), positive=True) or metrics["peak_rss_bytes"] > 256 * 1024 * 1024:
        raise EvidenceError("private shadow RSS bound failed")
    if not numeric(metrics.get("peak_open_file_descriptors"), positive=True):
        raise EvidenceError("private shadow descriptor evidence is invalid")
    forbidden = {"path", "name", "metadata", "coordinates", "latitude", "longitude", "content_sha256"}
    stack: list[object] = [report]
    while stack:
        value = stack.pop()
        if isinstance(value, dict):
            if forbidden.intersection(value):
                raise EvidenceError("private evidence contains a forbidden field")
            stack.extend(value.values())
        elif isinstance(value, list):
            stack.extend(value)
    return binary


def validate_soak(report: dict[str, Any], expected_binary: str) -> None:
    if report.get("schema") != "phase6-synthetic-soak-v1" or report.get("cleanup_verified") is not True:
        raise EvidenceError("synthetic soak schema or cleanup proof is invalid")
    revision, binary = validate_environment(report.get("environment"), "soak environment")
    if revision != "9d2490b301b8bfcd8f9d57ab97bde816e3a37201" or binary != expected_binary:
        raise EvidenceError("synthetic soak source or binary identity drift")
    fixture = object_value(report.get("fixture"), "synthetic fixture")
    expected = {
        "assets": 2500, "sidecars": 2500, "logical_media_bytes": 1310720000,
        "logical_sidecar_bytes": 282500, "logical_source_bytes": 1311002500,
        "generator": "phase6-deterministic-block-v1",
    }
    if any(fixture.get(key) != value for key, value in expected.items()):
        raise EvidenceError("synthetic fixture identity drift")
    if fixture.get("allocated_bytes", 0) < fixture["logical_source_bytes"]:
        raise EvidenceError("synthetic fixture was not fully allocated")
    digest(fixture.get("corpus_sha256"), "synthetic corpus")
    digest(report.get("normalized_plan_sha256"), "normalized soak plan")
    if report.get("plan_summary") != {
        "assets": 2500, "sidecars": 2500, "bytes_read": 1311002500,
    }:
        raise EvidenceError("synthetic soak plan counters drift")
    if report.get("methodology") != {
        "buffer_bytes": 65536,
        "warmups": 1,
        "retained_samples": 3,
        "cache_state": "one warmup before retained page-cached samples",
        "scope": "read-only Google Takeout scan and normalized plan",
    }:
        raise EvidenceError("synthetic soak methodology drift")
    samples = report.get("raw_samples")
    if not isinstance(samples, list) or len(samples) != 3:
        raise EvidenceError("synthetic soak retained sample count drift")
    for index, sample in enumerate(samples, start=1):
        value = object_value(sample, f"soak sample {index}")
        if value.get("sample") != index or any(not numeric(value.get(field)) for field in SAMPLE_METRICS):
            raise EvidenceError(f"soak sample {index} metrics drift")
        if value["characters_read"] < fixture["logical_source_bytes"]:
            raise EvidenceError(f"soak sample {index} did not stream the full source")
    aggregate = object_value(report.get("aggregate"), "soak aggregate")
    if aggregate.get("median_wall_time_seconds") != statistics.median(
        sample["wall_time_seconds"] for sample in samples
    ):
        raise EvidenceError("synthetic soak median drift")
    peak_rss = max(sample["peak_rss_bytes"] for sample in samples)
    peak_fds = max(sample["peak_open_file_descriptors"] for sample in samples)
    if aggregate.get("peak_rss_bytes") != peak_rss or peak_rss > 256 * 1024 * 1024:
        raise EvidenceError("synthetic soak RSS bound failed")
    if aggregate.get("peak_open_file_descriptors") != peak_fds:
        raise EvidenceError("synthetic soak descriptor aggregate drift")


def main() -> int:
    try:
        binary = validate_private(load(PRIVATE))
        validate_soak(load(SOAK), binary)
    except EvidenceError as error:
        print(f"Phase 6 evidence check failed: {error}", file=sys.stderr)
        return 1
    print("Phase 6 evidence passed: synthetic soak and authorized private shadow")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
