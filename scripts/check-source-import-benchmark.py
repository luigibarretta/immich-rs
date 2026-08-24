#!/usr/bin/env python3
"""Validate paired Apple Photos and Picasa import benchmark reports."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import statistics
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = ROOT / "docs/evidence"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
FIXTURE_SHA256 = "20f7ba19792221abf76034e44efb69fdac676bdf33700c947f05db8aaf3394ce"
CORPUS_SHA256 = "d0c52a6f70ea3bd58f1f5d5fe013ae74d4fee6199c4544b18a7d8b152ea00cb0"
METRICS = (
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds", "peak_rss_bytes",
    "peak_open_file_descriptors", "characters_read", "characters_written",
    "storage_bytes_read", "storage_bytes_written",
)
OPERATIONS = {"assets": 8, "media_bytes": 67_108_864, "retries": 0}


class EvidenceError(ValueError):
    """Source import benchmark evidence is incomplete or inconsistent."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, action="append")
    return parser.parse_args()


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def digest(value: object, label: str) -> None:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise EvidenceError(f"{label} is not a SHA-256 digest")


def validate_fixture(value: object) -> None:
    if value != {
        "id": "synthetic-source-import-64m", "kind": "synthetic",
        "license": "CC0-1.0", "manifest_sha256": FIXTURE_SHA256,
        "corpus_sha256": CORPUS_SHA256, "media_assets": 8,
        "media_bytes": 67_108_864, "metadata_updates": 0,
        "album_mutations": 0, "concurrency": 1,
    }:
        raise EvidenceError("benchmark fixture contract drifted")


def validate_tools(value: object) -> None:
    tools = object_value(value, "tools")
    rust = object_value(tools.get("immich_rs"), "immich-rs tool")
    oracle = object_value(tools.get("immich_go"), "oracle tool")
    if set(tools) != {"immich_rs", "immich_go"}:
        raise EvidenceError("benchmark tool set drifted")
    if not isinstance(rust.get("version"), str) or not rust["version"].startswith("immich-rs "):
        raise EvidenceError("immich-rs version is invalid")
    digest(rust.get("sha256"), "immich-rs binary")
    if oracle != {
        "name": "immich-go", "version": "0.32.0",
        "upstream_commit": "f7d19fce34acd4884ea2c02fc3025706a060afdf",
        "license": "AGPL-3.0", "archive": "immich-go_Linux_x86_64.tar.gz",
        "archive_sha256": "6e2ad86bafdadb9466d6515de7cb882726c0aea1a21d51164dff361d7d480a97",
        "binary_sha256": "eb46733ccf8ff78fb7207b9601695ff5730927cc712f08c6a93e315c845d5475",
        "sha256": "eb46733ccf8ff78fb7207b9601695ff5730927cc712f08c6a93e315c845d5475",
    }:
        raise EvidenceError("oracle identity drifted")


def validate_methodology(value: object) -> tuple[int, int]:
    method = object_value(value, "methodology")
    samples = method.get("samples")
    warmups = method.get("warmups")
    if not isinstance(samples, int) or isinstance(samples, bool) or not 2 <= samples <= 10:
        raise EvidenceError("sample count is outside its bound")
    if not isinstance(warmups, int) or isinstance(warmups, bool) or not 0 <= warmups <= 2:
        raise EvidenceError("warmup count is outside its bound")
    if method != {
        "samples": samples, "warmups": warmups,
        "pairing": "fresh isolated owner per tool on one disposable server and one corpus",
        "order": "alternating within pairs", "concurrency": 1,
        "scope": "complete plan plus import; setup and probes excluded",
        "compatibility_intersection": "uploads only; albums and metadata disabled",
        "percentiles": "nearest-rank p95 over retained samples",
    }:
        raise EvidenceError("benchmark methodology drifted")
    return samples, warmups


def metric_value(value: object, label: str) -> float | int:
    if not isinstance(value, (int, float)) or isinstance(value, bool) or value < 0:
        raise EvidenceError(f"{label} is invalid")
    return value


def validate_samples(value: object, count: int, warmups: int) -> list[dict[str, Any]]:
    if not isinstance(value, list) or len(value) != count:
        raise EvidenceError("raw sample count drifted")
    samples: list[dict[str, Any]] = []
    for index, sample in enumerate(value):
        pair = object_value(sample, f"sample {index}")
        first = "immich-rs-first" if (index + warmups) % 2 == 0 else "immich-go-first"
        if set(pair) != {"sample", "execution_order", "immich_rs", "immich_go"}:
            raise EvidenceError("raw sample schema drifted")
        if pair.get("sample") != index or pair.get("execution_order") != first:
            raise EvidenceError("sample ordering drifted")
        for tool in ("immich_rs", "immich_go"):
            measured = object_value(pair.get(tool), f"sample {index} {tool}")
            if set(measured) != {*METRICS, "operations"} or measured.get("operations") != OPERATIONS:
                raise EvidenceError("sample operation counters drifted")
            for metric in METRICS:
                metric_value(measured.get(metric), f"sample {index} {tool} {metric}")
        samples.append(pair)
    return samples


def expected_aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, object]:
    result = {}
    for metric in METRICS:
        values = sorted(sample[tool][metric] for sample in samples)
        index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[metric] = {
            "min": min(values), "median": statistics.median(values),
            "p95": values[index], "max": max(values),
        }
    return result


def validate_claims(value: object, aggregate: dict[str, Any], adapter: str) -> None:
    rust = aggregate["immich_rs"]["wall_time_seconds"]
    go = aggregate["immich_go"]["wall_time_seconds"]
    improvement = 100 * (go["median"] - rust["median"]) / go["median"]
    claim = "Raw measurements only; no performance improvement is claimed."
    if improvement >= 10 and rust["max"] < go["min"]:
        claim = (
            f"On this exact 67,108,864-byte eight-asset synthetic {adapter} import, "
            f"immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; "
            "this is not a large-library, WAN or production claim."
        )
    if value != [claim]:
        raise EvidenceError("performance claim is not justified by raw samples")


def validate(path: Path) -> str:
    try:
        report = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load benchmark: {error}") from error
    if set(report) != {"schema", "adapter", "manifest", "raw_samples", "aggregate", "claims"}:
        raise EvidenceError("benchmark schema drifted")
    adapter = report.get("adapter")
    if report.get("schema") != "source-import-benchmark-v1" or adapter not in {"apple-photos", "picasa"}:
        raise EvidenceError("benchmark identity drifted")
    manifest = object_value(report.get("manifest"), "manifest")
    if set(manifest) != {"source_revision", "fixture", "tools", "environment", "methodology"}:
        raise EvidenceError("benchmark manifest drifted")
    revision = manifest.get("source_revision")
    if not isinstance(revision, str) or COMMIT.fullmatch(revision) is None:
        raise EvidenceError("source revision is invalid")
    validate_fixture(manifest.get("fixture"))
    validate_tools(manifest.get("tools"))
    environment = object_value(manifest.get("environment"), "environment")
    if environment.get("system") != "Linux" or environment.get("architecture") != "x86_64" or environment.get("hostname") != "<REDACTED_HOST>":
        raise EvidenceError("benchmark environment drifted")
    count, warmups = validate_methodology(manifest.get("methodology"))
    samples = validate_samples(report.get("raw_samples"), count, warmups)
    expected = {tool: expected_aggregate(samples, tool) for tool in ("immich_rs", "immich_go")}
    if report.get("aggregate") != expected:
        raise EvidenceError("benchmark aggregate does not match raw samples")
    validate_claims(report.get("claims"), expected, adapter)
    encoded = json.dumps(report, sort_keys=True)
    if any(token in encoded for token in ("/tmp/", "127.0.0.1", "example.invalid", "accessToken", "api_key")):
        raise EvidenceError("benchmark leaked sensitive or host-specific material")
    return adapter


def main() -> int:
    requested = arguments().input
    paths = requested or sorted(EVIDENCE_ROOT.glob("phase*-source-import-benchmark-*.json"))
    try:
        if not paths:
            raise EvidenceError("source import benchmark evidence is missing")
        adapters = {validate(path) for path in paths}
        if requested is None and adapters != {"apple-photos", "picasa"}:
            raise EvidenceError("Apple Photos and Picasa benchmark evidence are both required")
    except EvidenceError as error:
        print(f"source import benchmark check failed: {error}", file=sys.stderr)
        return 1
    print(f"source import benchmark evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
