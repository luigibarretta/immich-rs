#!/usr/bin/env python3
"""Validate paired raw Phase 8 Takeout benchmark evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics
import sys
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_INPUT = ROOT / "benchmarks/evidence/phase8-2026-08-24.json"
FIXTURE = ROOT / "benchmarks/fixtures/phase8-takeout-64m.json"
BASELINE = ROOT / "tests/oracle/baseline.toml"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
RUST_VERSION = re.compile(
    r"^immich-rs (?:0\.0\.0|[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[1-9][0-9]*)?)$"
)
METRICS = {
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds", "peak_rss_bytes",
    "peak_open_file_descriptors", "characters_read", "characters_written",
    "storage_bytes_read", "storage_bytes_written",
}
OPERATIONS = {"source_operations": 8, "metadata_assignments": 8, "visible_assets": 8, "retries": 0}
CORPUS_SHA256 = "54c2a3189642c8c26c1c65b28be285df0154d3b06957f349a0cf2e815aeb697f"


class EvidenceError(ValueError):
    """The Phase 8 benchmark report is incomplete or inconsistent."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    return parser.parse_args()


def load(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read evidence: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError("benchmark evidence must be an object")
    return value


def numeric(value: object) -> bool:
    return not isinstance(value, bool) and isinstance(value, (int, float)) and value >= 0


def aggregate(samples: list[dict[str, Any]], tool: str, field: str) -> dict[str, Any]:
    values = sorted(sample[tool][field] for sample in samples)
    return {
        "min": min(values),
        "median": statistics.median(values),
        "p95": values[-1],
        "max": max(values),
    }


def validate_manifest(report: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    manifest = report.get("manifest")
    if not isinstance(manifest, dict) or COMMIT.fullmatch(str(manifest.get("source_revision"))) is None:
        raise EvidenceError("source revision is invalid")
    fixture = manifest.get("fixture")
    expected_fixture = {
        "id": "synthetic-phase8-takeout-64m", "kind": "synthetic", "license": "CC0-1.0",
        "manifest_sha256": hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
        "corpus_sha256": CORPUS_SHA256, "media_assets": 8, "metadata_updates": 8,
        "media_bytes": 67_108_864, "concurrency": 1,
    }
    if fixture != expected_fixture:
        raise EvidenceError("fixture identity or dimensions drifted")
    environment = manifest.get("environment")
    if not isinstance(environment, dict) or environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError("environment hostname is not redacted")
    methodology = manifest.get("methodology")
    if not isinstance(methodology, dict) or methodology != {
        "samples": 6, "warmups": 2,
        "pairing": "fresh isolated owner per tool on one disposable HTTPS server and one corpus",
        "order": "alternating within pairs",
        "scope": "complete plan plus import; account setup, server startup and postcondition probes excluded",
        "concurrency": 1, "percentiles": "nearest-rank p95 over the raw samples",
    }:
        raise EvidenceError("benchmark methodology drifted")
    return manifest, methodology


def validate_tools(manifest: dict[str, Any]) -> None:
    tools = manifest.get("tools")
    rust = tools.get("immich_rs") if isinstance(tools, dict) else None
    oracle = tools.get("immich_go") if isinstance(tools, dict) else None
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    expected = baseline["artifacts"]["linux_x86_64"]["binary_sha256"]
    version = rust.get("version") if isinstance(rust, dict) else None
    if not isinstance(version, str) or RUST_VERSION.fullmatch(version) is None:
        raise EvidenceError("immich-rs identity drifted")
    if not isinstance(rust.get("sha256"), str) or SHA256.fullmatch(rust["sha256"]) is None:
        raise EvidenceError("immich-rs digest is invalid")
    if not isinstance(oracle, dict) or oracle.get("version") != "0.32.0":
        raise EvidenceError("oracle version drifted")
    if oracle.get("binary_sha256") != expected or oracle.get("sha256") != expected:
        raise EvidenceError("oracle digest drifted")


def validate_samples(report: dict[str, Any]) -> list[dict[str, Any]]:
    samples = report.get("raw_samples")
    if not isinstance(samples, list) or len(samples) != 6:
        raise EvidenceError("six retained samples are required")
    for index, sample in enumerate(samples):
        order = "immich-rs-first" if index % 2 == 0 else "immich-go-first"
        if not isinstance(sample, dict) or sample.get("sample") != index or sample.get("execution_order") != order:
            raise EvidenceError("paired sample order drifted")
        for tool in ("immich_rs", "immich_go"):
            measured = sample.get(tool)
            if not isinstance(measured, dict) or set(measured) != METRICS | {"operations"}:
                raise EvidenceError("sample metric set drifted")
            if any(not numeric(measured.get(field)) for field in METRICS):
                raise EvidenceError("sample metric is invalid")
            if measured.get("operations") != OPERATIONS:
                raise EvidenceError("server outcomes are not comparable")
    return samples


def validate_claim(report: dict[str, Any], samples: list[dict[str, Any]]) -> None:
    aggregates = report.get("aggregate")
    if not isinstance(aggregates, dict):
        raise EvidenceError("aggregates are missing")
    for tool in ("immich_rs", "immich_go"):
        if set(aggregates.get(tool, {})) != METRICS:
            raise EvidenceError("aggregate metric set drifted")
        for field in METRICS:
            if aggregates[tool][field] != aggregate(samples, tool, field):
                raise EvidenceError(f"aggregate drifted for {tool}.{field}")
    rust_wall = aggregates["immich_rs"]["wall_time_seconds"]
    go_wall = aggregates["immich_go"]["wall_time_seconds"]
    if rust_wall["p95"] > go_wall["p95"] * 1.1:
        raise EvidenceError("ADR-0012 p95 budget failed")
    if aggregates["immich_rs"]["peak_rss_bytes"]["max"] > 256 * 1024 * 1024:
        raise EvidenceError("client RSS budget failed")
    improvement = 100 * (go_wall["median"] - rust_wall["median"]) / go_wall["median"]
    raw_only = "Raw measurements only; no performance improvement is claimed."
    expected = raw_only
    if improvement >= 10 and rust_wall["max"] < go_wall["min"]:
        expected = (
            "On this exact 67,108,864-byte eight-asset synthetic Takeout import, "
            f"immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; "
            "this is not a large-library, WAN or production claim."
        )
    if report.get("claims") != [expected]:
        raise EvidenceError("performance claim is unsupported")


def validate(path: Path) -> None:
    report = load(path)
    encoded = json.dumps(report, sort_keys=True)
    forbidden = ("/tmp/", "example.invalid", "accessToken", "api_key", "password", "backup-reference")
    if report.get("schema") != "phase8-benchmark-report-v1" or any(value in encoded for value in forbidden):
        raise EvidenceError("schema or redaction contract drifted")
    manifest, _methodology = validate_manifest(report)
    validate_tools(manifest)
    samples = validate_samples(report)
    validate_claim(report, samples)


def main() -> int:
    try:
        validate(arguments().input)
    except (EvidenceError, OSError, UnicodeError, ValueError, KeyError) as error:
        print(f"Phase 8 benchmark evidence check failed: {error}", file=sys.stderr)
        return 1
    print("Phase 8 paired benchmark evidence passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
