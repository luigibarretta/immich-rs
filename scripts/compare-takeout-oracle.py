#!/usr/bin/env python3
"""Compare a Google Takeout plan with one normalized black-box observation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EXPECTATION_ROOT = (REPOSITORY_ROOT / "tests" / "oracle" / "compatibility").resolve()
PLAN_ROOT = (REPOSITORY_ROOT / "tests" / "fixtures" / "v1").resolve()
EXPECTED_CHECKS = {
    "decompressed_layout",
    "json_title_match",
    "metadata_association",
    "dry_run_mutations",
}


class TakeoutDifferentialError(ValueError):
    """The Takeout observation does not satisfy the declared matrix."""


def _json_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise TakeoutDifferentialError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise TakeoutDifferentialError(f"{label} must be a JSON object")
    return value


def _load_expectation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = path.resolve()
    if EXPECTATION_ROOT not in resolved.parents:
        raise TakeoutDifferentialError("expectation must be inside the compatibility root")
    expectation = _json_object(resolved, "Takeout differential expectation")
    if expectation.get("schema") != "takeout-differential-expectation-v1":
        raise TakeoutDifferentialError("unsupported Takeout expectation schema")
    raw_plan = expectation.get("expected_plan")
    if not isinstance(raw_plan, str):
        raise TakeoutDifferentialError("expected_plan must be a relative string")
    plan_path = (resolved.parent / raw_plan).resolve()
    if PLAN_ROOT not in plan_path.parents or plan_path.name != "expected-plan.json":
        raise TakeoutDifferentialError("expected plan must be a v1 synthetic golden")
    matrix = expectation.get("matrix")
    if not isinstance(matrix, list) or any(
        not isinstance(item, dict)
        or set(item) != {"id", "outcome"}
        or not isinstance(item["id"], str)
        or not isinstance(item["outcome"], str)
        for item in matrix
    ):
        raise TakeoutDifferentialError("matrix entries are malformed")
    identifiers = [item["id"] for item in matrix]
    if len(identifiers) != len(set(identifiers)) or set(identifiers) != EXPECTED_CHECKS:
        raise TakeoutDifferentialError("matrix must declare every Takeout check once")
    return expectation, plan_path


def _log_lines(observation: dict[str, Any]) -> list[str]:
    process = observation.get("process")
    logs = process.get("logs") if isinstance(process, dict) else None
    if not isinstance(logs, list):
        raise TakeoutDifferentialError("observation does not contain normalized logs")
    lines: list[str] = []
    for log in logs:
        if not isinstance(log, dict) or not isinstance(log.get("lines"), list):
            raise TakeoutDifferentialError("oracle log entry is malformed")
        if not all(isinstance(line, str) for line in log["lines"]):
            raise TakeoutDifferentialError("oracle log lines must be strings")
        lines.extend(log["lines"])
    return lines


def _paths(lines: list[str], marker: str) -> set[str]:
    pattern = re.compile(
        rf"\b{re.escape(marker)} file=fixture:(.+?)(?:\s+(?:type|title|date|reason)=.*)?$"
    )
    return {match.group(1) for line in lines if (match := pattern.search(line))}


def _plan_facts(plan: dict[str, Any]) -> tuple[set[str], set[str], set[str]]:
    source = plan.get("source")
    assets = plan.get("assets")
    if not isinstance(source, dict) or source.get("kind") != "google_takeout":
        raise TakeoutDifferentialError("golden is not a Google Takeout plan")
    if not isinstance(assets, list) or any(not isinstance(asset, dict) for asset in assets):
        raise TakeoutDifferentialError("golden assets are malformed")
    asset_paths: set[str] = set()
    metadata_paths: set[str] = set()
    metadata_assets: set[str] = set()
    for asset in assets:
        path = asset.get("relative_path")
        metadata = asset.get("metadata")
        if not isinstance(path, str) or not isinstance(metadata, list):
            raise TakeoutDifferentialError("golden asset fields are malformed")
        asset_paths.add(path)
        if metadata:
            metadata_assets.add(path)
        for sidecar in metadata:
            if (
                not isinstance(sidecar, dict)
                or not isinstance(sidecar.get("relative_path"), str)
                or sidecar.get("rule_id") != "META_GOOGLE_TAKEOUT_TITLE_V1"
            ):
                raise TakeoutDifferentialError("golden Takeout metadata is malformed")
            metadata_paths.add(sidecar["relative_path"])
    if plan.get("warnings") or plan.get("errors"):
        raise TakeoutDifferentialError("basic Takeout golden must be diagnostic-free")
    return asset_paths, metadata_paths, metadata_assets


def compare(expectation_path: Path, observation_path: Path) -> dict[str, Any]:
    """Compare exact normalized Takeout facts and return a stable report."""
    expectation, plan_path = _load_expectation(expectation_path)
    plan = _json_object(plan_path, "expected normalized plan")
    observation = _json_object(observation_path, "oracle observation")
    if observation.get("schema") != "oracle-observation-v1":
        raise TakeoutDifferentialError("unsupported oracle observation schema")
    if observation.get("case_id") != expectation.get("case_id"):
        raise TakeoutDifferentialError("oracle case does not match expectation")
    process = observation.get("process")
    observable = observation.get("observable")
    if not isinstance(process, dict) or not isinstance(observable, dict):
        raise TakeoutDifferentialError("oracle observation is incomplete")
    assets, sidecars, metadata_assets = _plan_facts(plan)
    lines = _log_lines(observation)
    discovered = _paths(lines, "discovered image") | _paths(lines, "discovered video")
    uploaded = _paths(lines, "uploaded successfully")
    discovered_sidecars = _paths(lines, "discovered sidecar")
    metadata_updated = _paths(lines, "metadata updated")
    commits = observable.get("committed_mutations")
    if not isinstance(commits, list):
        raise TakeoutDifferentialError("oracle mutation set is malformed")
    checks = {
        "decompressed_layout": bool(assets) and discovered == assets and uploaded == assets,
        "json_title_match": bool(sidecars) and discovered_sidecars == sidecars,
        "metadata_association": metadata_updated == metadata_assets,
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
        raise TakeoutDifferentialError("oracle process or mock authentication failed")
    failed = sorted(check_id for check_id, passed in checks.items() if not passed)
    if failed:
        raise TakeoutDifferentialError("Takeout differential checks failed: " + ", ".join(failed))
    outcomes = {item["id"]: item["outcome"] for item in expectation["matrix"]}
    oracle = observation.get("oracle")
    version = oracle.get("version") if isinstance(oracle, dict) else None
    return {
        "schema": "takeout-differential-report-v1",
        "case_id": expectation["case_id"],
        "oracle_version": version,
        "checks": [
            {"id": check_id, "outcome": outcomes[check_id], "passed": True}
            for check_id in sorted(checks)
        ],
        "counts": {
            "planned_assets": len(assets),
            "oracle_unique_assets": len(uploaded),
            "sidecars": len(sidecars),
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
    except (TakeoutDifferentialError, OSError, UnicodeError) as error:
        print(f"Takeout differential comparison failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
