#!/usr/bin/env python3
"""Compare Apple Photos v3 facts with a normalized immich-go observation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EXPECTATION_ROOT = (REPOSITORY_ROOT / "tests/oracle/compatibility").resolve()
PLAN_ROOT = (REPOSITORY_ROOT / "tests/fixtures/v3").resolve()
EXPECTED_CHECKS = {
    "asset_discovery", "dry_run_mutations", "known_export_noise", "live_photo_pair",
    "preserve_all_variants", "split_zip_layout", "unicode_nfc", "xmp_sidecar",
}
FILE_PATTERN = re.compile(r"\bfile=icloud-\d+:(.+?)(?:\s+(?:reason|type|date)=.*)?$")


class DifferentialError(ValueError):
    """The Apple Photos observation does not satisfy its declared matrix."""


def _json_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DifferentialError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise DifferentialError(f"{label} must be a JSON object")
    return value


def _load_expectation(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = path.resolve()
    if EXPECTATION_ROOT not in resolved.parents:
        raise DifferentialError("expectation must be inside the compatibility root")
    expectation = _json_object(resolved, "Apple expectation")
    if expectation.get("schema") != "apple-differential-expectation-v1":
        raise DifferentialError("unsupported Apple expectation schema")
    raw_plan = expectation.get("expected_plan")
    if not isinstance(raw_plan, str):
        raise DifferentialError("expected_plan must be a relative string")
    plan_path = (resolved.parent / raw_plan).resolve()
    if PLAN_ROOT not in plan_path.parents or plan_path.name != "expected-plan.json":
        raise DifferentialError("expected plan must be a v3 synthetic golden")
    matrix = expectation.get("matrix")
    if not isinstance(matrix, list) or any(
        not isinstance(item, dict)
        or set(item) != {"id", "outcome"}
        or not isinstance(item["id"], str)
        or not isinstance(item["outcome"], str)
        for item in matrix
    ):
        raise DifferentialError("matrix entries are malformed")
    identifiers = [item["id"] for item in matrix]
    if len(identifiers) != len(set(identifiers)) or set(identifiers) != EXPECTED_CHECKS:
        raise DifferentialError("matrix must declare every Apple check once")
    return expectation, plan_path


def _process_lines(observation: dict[str, Any]) -> list[str]:
    process = observation.get("process")
    if not isinstance(process, dict):
        raise DifferentialError("observation process is malformed")
    stdout = process.get("stdout", [])
    logs = process.get("logs", [])
    if not isinstance(stdout, list) or not isinstance(logs, list):
        raise DifferentialError("observation text capture is malformed")
    lines = [line for line in stdout if isinstance(line, str)]
    for log in logs:
        if not isinstance(log, dict) or not isinstance(log.get("lines"), list):
            raise DifferentialError("observation log is malformed")
        lines.extend(line for line in log["lines"] if isinstance(line, str))
    return lines


def _paths(lines: list[str], marker: str) -> set[str]:
    paths = set()
    for line in lines:
        if marker in line and (matched := FILE_PATTERN.search(line)):
            paths.add(matched.group(1))
    return paths


def _plan_paths(plan: dict[str, Any]) -> tuple[set[str], set[str]]:
    assets = plan.get("assets")
    if plan.get("schema_version") != 3 or not isinstance(assets, list):
        raise DifferentialError("golden is not normalized-plan-v3")
    paths = set()
    sidecars = set()
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("relative_path"), str):
            raise DifferentialError("golden asset is malformed")
        paths.add(asset["relative_path"])
        metadata = asset.get("metadata")
        if not isinstance(metadata, list):
            raise DifferentialError("golden metadata candidates are malformed")
        sidecars.update(
            item["relative_path"]
            for item in metadata
            if isinstance(item, dict) and isinstance(item.get("relative_path"), str)
        )
    return paths, sidecars


def _contained_mutations(commits: list[object]) -> bool:
    return len(commits) == 5 and all(
        isinstance(commit, dict)
        and commit.get("method") == "PUT"
        and isinstance(commit.get("path"), str)
        and commit["path"].startswith("/api/jobs/")
        for commit in commits
    )


def compare(expectation_path: Path, observation_path: Path) -> dict[str, Any]:
    """Compare normalized Apple facts and emit a stable report."""
    expectation, plan_path = _load_expectation(expectation_path)
    plan = _json_object(plan_path, "expected normalized plan")
    observation = _json_object(observation_path, "oracle observation")
    if observation.get("schema") != "oracle-observation-v1" or observation.get(
        "case_id"
    ) != expectation.get("case_id"):
        raise DifferentialError("oracle observation does not match the expectation")
    process = observation.get("process")
    observable = observation.get("observable")
    fixture = observation.get("fixture")
    if not all(isinstance(item, dict) for item in (process, observable, fixture)):
        raise DifferentialError("oracle observation is incomplete")
    if process.get("exit_code") != 0 or observable.get("all_requests_authenticated") is not True:
        raise DifferentialError("oracle process or mock authentication failed")
    planned, planned_sidecars = _plan_paths(plan)
    lines = _process_lines(observation)
    discovered = _paths(lines, "discovered image file=") | _paths(lines, "discovered video file=")
    uploaded = _paths(lines, "uploaded successfully file=")
    sidecars = _paths(lines, "discovered sidecar file=")
    stacked = _paths(lines, "stacked file=")
    banned = _paths(lines, "discovered banned file file=")
    commits = observable.get("committed_mutations")
    if not isinstance(commits, list):
        raise DifferentialError("oracle mutation set is malformed")
    pair = {
        "Albums/Synthetic Journey/pair.mov",
        "Albums/Synthetic Journey/pair.png",
    }
    variants = {
        "Albums/Synthetic Journey/rendered-edited.png",
        "Albums/Synthetic Journey/rendered.png",
    }
    checks = {
        "split_zip_layout": fixture.get("archive_view") == "icloud-split"
        and any("icloud-001:" in line for line in lines)
        and any("icloud-002:" in line for line in lines),
        "asset_discovery": discovered == planned and uploaded == planned,
        "xmp_sidecar": sidecars == planned_sidecars,
        "live_photo_pair": stacked == pair,
        "preserve_all_variants": variants <= discovered and variants <= uploaded,
        "known_export_noise": banned == {".DS_Store", "Recently Deleted"},
        "unicode_nfc": "Albums/Synthetic Journey/café.png" in discovered,
        "dry_run_mutations": _contained_mutations(commits)
        and not any(
            isinstance(commit, dict) and commit.get("path") == "/api/assets"
            for commit in commits
        ),
    }
    failed = sorted(identifier for identifier, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("Apple differential checks failed: " + ", ".join(failed))
    outcomes = {item["id"]: item["outcome"] for item in expectation["matrix"]}
    oracle = observation.get("oracle")
    return {
        "schema": "apple-differential-report-v1",
        "case_id": expectation["case_id"],
        "oracle_version": oracle.get("version") if isinstance(oracle, dict) else None,
        "checks": [
            {"id": identifier, "outcome": outcomes[identifier], "passed": True}
            for identifier in sorted(checks)
        ],
        "counts": {
            "planned_assets": len(planned),
            "oracle_assets": len(discovered),
            "sidecars": len(planned_sidecars),
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
        print(f"Apple differential comparison failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
