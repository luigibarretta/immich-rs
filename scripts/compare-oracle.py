#!/usr/bin/env python3
"""Compare a normalized immich-go observation with an immich-rs golden plan."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any
import unicodedata


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EXPECTATION_ROOT = (REPOSITORY_ROOT / "tests" / "oracle" / "compatibility").resolve()
PLAN_ROOT = (REPOSITORY_ROOT / "tests" / "fixtures" / "v1").resolve()
EXPECTED_CHECKS = {
    "regular_recursive",
    "unicode_nfc",
    "case_collision",
    "duplicate_basename",
    "xmp_sidecar",
    "generic_json_sidecar",
    "live_photo_pair",
    "symlink",
    "dry_run_mutations",
}


class DifferentialError(ValueError):
    """The observation does not satisfy the declared compatibility matrix."""


def _json_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DifferentialError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise DifferentialError(f"{label} must be a JSON object")
    return value


def load_expectation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = path.resolve()
    if EXPECTATION_ROOT not in resolved.parents:
        raise DifferentialError("expectation must be inside tests/oracle/compatibility")
    expectation = _json_object(resolved, "differential expectation")
    if expectation.get("schema") != "differential-expectation-v1":
        raise DifferentialError("unsupported differential expectation schema")
    raw_plan = expectation.get("expected_plan")
    if not isinstance(raw_plan, str):
        raise DifferentialError("expected_plan must be a relative string")
    plan_path = (resolved.parent / raw_plan).resolve()
    if PLAN_ROOT not in plan_path.parents or plan_path.name != "expected-plan.json":
        raise DifferentialError("expected plan must be a v1 synthetic golden plan")
    matrix = expectation.get("matrix")
    if not isinstance(matrix, list) or any(
        not isinstance(item, dict)
        or set(item) != {"id", "outcome"}
        or not isinstance(item["id"], str)
        or not isinstance(item["outcome"], str)
        for item in matrix
    ):
        raise DifferentialError("matrix entries must contain string id and outcome fields")
    identifiers = [item["id"] for item in matrix]
    if len(identifiers) != len(set(identifiers)) or set(identifiers) != EXPECTED_CHECKS:
        raise DifferentialError("compatibility matrix must declare every differential check once")
    return expectation, plan_path


def _log_lines(observation: dict[str, Any]) -> list[str]:
    process = observation.get("process")
    logs = process.get("logs") if isinstance(process, dict) else None
    if not isinstance(logs, list):
        raise DifferentialError("oracle observation does not contain normalized logs")
    lines: list[str] = []
    for log in logs:
        if not isinstance(log, dict) or not isinstance(log.get("lines"), list):
            raise DifferentialError("oracle log entry is malformed")
        if not all(isinstance(line, str) for line in log["lines"]):
            raise DifferentialError("oracle log lines must be strings")
        lines.extend(log["lines"])
    return lines


def _paths(lines: list[str], marker: str) -> set[str]:
    pattern = re.compile(rf"\b{re.escape(marker)} file=fixture:(.+?)(?: reason=.*)?$")
    return {match.group(1) for line in lines if (match := pattern.search(line))}


def _diagnostic_paths(plan: dict[str, Any], collection: str, rule_id: str) -> set[str]:
    diagnostics = plan.get(collection)
    if not isinstance(diagnostics, list):
        raise DifferentialError(f"plan {collection} must be an array")
    paths: set[str] = set()
    for diagnostic in diagnostics:
        if isinstance(diagnostic, dict) and diagnostic.get("rule_id") == rule_id:
            values = diagnostic.get("paths")
            if isinstance(values, list) and all(isinstance(value, str) for value in values):
                paths.update(values)
    return paths


def _plan_facts(plan: dict[str, Any]) -> dict[str, set[str]]:
    assets = plan.get("assets")
    if not isinstance(assets, list) or any(not isinstance(asset, dict) for asset in assets):
        raise DifferentialError("normalized plan assets are malformed")
    asset_paths = {asset.get("relative_path") for asset in assets}
    if None in asset_paths or any(not isinstance(path, str) for path in asset_paths):
        raise DifferentialError("normalized plan contains an invalid asset path")
    metadata_paths = {
        metadata.get("relative_path")
        for asset in assets
        for metadata in asset.get("metadata", [])
        if isinstance(metadata, dict)
    }
    if None in metadata_paths or any(not isinstance(path, str) for path in metadata_paths):
        raise DifferentialError("normalized plan contains an invalid metadata path")
    return {
        "assets": asset_paths,
        "metadata": metadata_paths,
        "live": {asset["relative_path"] for asset in assets if asset.get("live_photo") is not None},
        "symlinks": _diagnostic_paths(plan, "warnings", "FS_SYMLINK_SKIPPED_V1"),
        "orphans": _diagnostic_paths(plan, "warnings", "META_ORPHAN_SIDECAR_V1"),
        "case": _diagnostic_paths(plan, "errors", "FS_CASE_COLLISION_V1"),
        "duplicates": _diagnostic_paths(plan, "warnings", "FS_DUPLICATE_BASENAME_V1"),
    }


def compare(expectation_path: Path, observation_path: Path) -> dict[str, Any]:
    expectation, plan_path = load_expectation(expectation_path)
    plan = _json_object(plan_path, "expected normalized plan")
    observation = _json_object(observation_path, "oracle observation")
    if observation.get("schema") != "oracle-observation-v1":
        raise DifferentialError("unsupported oracle observation schema")
    if observation.get("case_id") != expectation.get("case_id"):
        raise DifferentialError("oracle observation case does not match expectation")
    process = observation.get("process")
    observable = observation.get("observable")
    if not isinstance(process, dict) or not isinstance(observable, dict):
        raise DifferentialError("oracle observation is incomplete")
    facts = _plan_facts(plan)
    lines = _log_lines(observation)
    discovered_assets = _paths(lines, "discovered image") | _paths(lines, "discovered video")
    uploaded = _paths(lines, "uploaded successfully")
    discovered_sidecars = _paths(lines, "discovered sidecar")
    stacked = _paths(lines, "stacked")
    discarded_duplicates = _paths(lines, "discarded local duplicate")
    rejected_json = _paths(lines, "JSON file detected but not from immich-go")
    json_candidates = {path for path in facts["metadata"] if path.endswith(".json")}
    xmp_candidates = {path for path in facts["metadata"] if path.endswith(".xmp")}
    commits = observable.get("committed_mutations")
    if not isinstance(commits, list):
        raise DifferentialError("oracle committed mutation set is malformed")
    checks = {
        "regular_recursive": uploaded == facts["assets"]
        and discovered_assets == facts["assets"] | facts["symlinks"]
        and discovered_sidecars == facts["metadata"] | facts["orphans"],
        "unicode_nfc": uploaded == facts["assets"]
        and all(path == unicodedata.normalize("NFC", path) for path in uploaded),
        "case_collision": bool(facts["case"]) and facts["case"] <= uploaded,
        "duplicate_basename": bool(facts["duplicates"]) and facts["duplicates"] <= uploaded,
        "xmp_sidecar": bool(xmp_candidates) and xmp_candidates <= discovered_sidecars,
        "generic_json_sidecar": bool(json_candidates) and json_candidates == rejected_json,
        "live_photo_pair": bool(facts["live"]) and facts["live"] == stacked,
        "symlink": bool(facts["symlinks"])
        and facts["symlinks"] == discarded_duplicates
        and facts["symlinks"].isdisjoint(facts["assets"]),
        "dry_run_mutations": bool(commits)
        and all(
            isinstance(commit, dict)
            and commit.get("method") == "PUT"
            and isinstance(commit.get("path"), str)
            and commit["path"].startswith("/api/jobs/")
            for commit in commits
        )
        and not any(isinstance(commit, dict) and commit.get("path") == "/api/assets" for commit in commits),
    }
    if process.get("exit_code") != 0 or observable.get("all_requests_authenticated") is not True:
        raise DifferentialError("oracle process or mock authentication did not succeed")
    failed = sorted(check_id for check_id, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("differential checks failed: " + ", ".join(failed))
    outcomes = {item["id"]: item["outcome"] for item in expectation["matrix"]}
    oracle = observation.get("oracle")
    oracle_version = oracle.get("version") if isinstance(oracle, dict) else None
    return {
        "schema": "differential-report-v1",
        "case_id": expectation["case_id"],
        "oracle_version": oracle_version,
        "checks": [
            {"id": check_id, "outcome": outcomes[check_id], "passed": True}
            for check_id in sorted(checks)
        ],
        "counts": {
            "planned_assets": len(facts["assets"]),
            "oracle_discovered_assets": len(discovered_assets),
            "oracle_unique_assets": len(uploaded),
            "sidecars": len(discovered_sidecars),
            "contained_oracle_mutations": len(commits),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expectation", type=Path)
    parser.add_argument("observation", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        report = compare(arguments.expectation, arguments.observation)
    except (DifferentialError, OSError, UnicodeError) as error:
        print(f"differential comparison failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
