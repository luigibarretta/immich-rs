#!/usr/bin/env python3
"""Compare Picasa v4 facts with directory and ZIP immich-go observations."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EXPECTATION_ROOT = (REPOSITORY_ROOT / "tests/oracle/compatibility").resolve()
PLAN_ROOT = (REPOSITORY_ROOT / "tests/fixtures/v4").resolve()
EXPECTED_CHECKS = {
    "directory_asset_discovery", "dry_run_mutations", "filename_date", "live_photo_pair",
    "picasa_album", "picasa_caption", "xmp_sidecar", "zip_asset_discovery",
}
FILE_PATTERN = re.compile(r"\bfile=[^:]+:(.+?)(?:\s+(?:reason|type|date)=.*)?$")


class DifferentialError(ValueError):
    """The Picasa observations do not satisfy the declared matrix."""


def _object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DifferentialError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise DifferentialError(f"{label} must be a JSON object")
    return value


def _expectation(path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    resolved = path.resolve()
    if EXPECTATION_ROOT not in resolved.parents:
        raise DifferentialError("expectation must be inside the compatibility root")
    expectation = _object(resolved, "Picasa expectation")
    if expectation.get("schema") != "picasa-differential-expectation-v1":
        raise DifferentialError("unsupported Picasa expectation schema")
    raw_plan = expectation.get("expected_plan")
    if not isinstance(raw_plan, str):
        raise DifferentialError("expected_plan must be relative")
    plan_path = (resolved.parent / raw_plan).resolve()
    if PLAN_ROOT not in plan_path.parents or plan_path.name != "expected-plan.json":
        raise DifferentialError("expected plan must be a v4 synthetic golden")
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
        raise DifferentialError("matrix must declare every Picasa check once")
    return expectation, _object(plan_path, "expected normalized plan")


def _lines(observation: dict[str, Any]) -> list[str]:
    process = observation.get("process")
    if not isinstance(process, dict):
        raise DifferentialError("observation process is malformed")
    lines = [line for line in process.get("stdout", []) if isinstance(line, str)]
    logs = process.get("logs", [])
    if not isinstance(logs, list):
        raise DifferentialError("observation logs are malformed")
    for log in logs:
        if not isinstance(log, dict) or not isinstance(log.get("lines"), list):
            raise DifferentialError("observation log is malformed")
        lines.extend(line for line in log["lines"] if isinstance(line, str))
    return lines


def _paths(lines: list[str], marker: str) -> set[str]:
    return {
        matched.group(1)
        for line in lines
        if marker in line and (matched := FILE_PATTERN.search(line))
    }


def _observation(path: Path, case_id: str) -> tuple[dict[str, Any], list[str]]:
    observation = _object(path, f"{case_id} observation")
    process = observation.get("process")
    observable = observation.get("observable")
    if (
        observation.get("schema") != "oracle-observation-v1"
        or observation.get("case_id") != case_id
        or not isinstance(process, dict)
        or not isinstance(observable, dict)
        or process.get("exit_code") != 0
        or observable.get("all_requests_authenticated") is not True
    ):
        raise DifferentialError(f"{case_id} observation is incomplete or failed")
    return observation, _lines(observation)


def _contained_mutations(observation: dict[str, Any]) -> bool:
    observable = observation.get("observable")
    commits = observable.get("committed_mutations") if isinstance(observable, dict) else None
    return isinstance(commits, list) and len(commits) == 5 and all(
        isinstance(commit, dict)
        and commit.get("method") == "PUT"
        and isinstance(commit.get("path"), str)
        and commit["path"].startswith("/api/jobs/")
        for commit in commits
    )


def compare(expectation_path: Path, directory_path: Path, zip_path: Path) -> dict[str, Any]:
    """Compare normalized Picasa facts and emit a stable compatibility report."""
    expectation, plan = _expectation(expectation_path)
    case_ids = expectation.get("case_ids")
    if case_ids != ["picasa-directory-v4", "picasa-v4"]:
        raise DifferentialError("Picasa case IDs are not exact")
    directory, directory_lines = _observation(directory_path, case_ids[0])
    archived, archive_lines = _observation(zip_path, case_ids[1])
    assets = plan.get("assets")
    if plan.get("schema_version") != 4 or not isinstance(assets, list):
        raise DifferentialError("golden is not normalized-plan-v4")
    planned = {asset.get("relative_path") for asset in assets if isinstance(asset, dict)}
    if None in planned or len(planned) != len(assets):
        raise DifferentialError("golden asset paths are malformed")
    sidecars = {
        item.get("relative_path")
        for asset in assets if isinstance(asset, dict)
        for item in asset.get("metadata", []) if isinstance(item, dict)
    }
    dated = "Albums/Synthetic/20240102-030405-image.png"
    captioned = "Albums/Synthetic/captioned.png"
    pair = {"Albums/Synthetic/pair.mov", "Albums/Synthetic/pair.png"}
    observed = [
        _paths(directory_lines, "discovered image file=")
        | _paths(directory_lines, "discovered video file="),
        _paths(archive_lines, "discovered image file=")
        | _paths(archive_lines, "discovered video file="),
    ]
    metadata_updates = [
        _paths(directory_lines, "metadata updated file="),
        _paths(archive_lines, "metadata updated file="),
    ]
    directory_stacked = _paths(directory_lines, "stacked file=")
    archive_stacked = _paths(archive_lines, "stacked file=")
    checks = {
        "directory_asset_discovery": observed[0] == planned,
        "zip_asset_discovery": observed[1] == planned,
        "xmp_sidecar": _paths(directory_lines, "discovered sidecar file=") == sidecars
        and _paths(archive_lines, "discovered sidecar file=") == sidecars,
        "filename_date": metadata_updates == [{dated}, {dated}],
        "picasa_caption": any(
            isinstance(asset, dict)
            and asset.get("relative_path") == captioned
            and isinstance(asset.get("normalized_metadata"), dict)
            and asset["normalized_metadata"].get("description") == "Synthetic caption"
            for asset in assets
        ) and all(captioned not in updates for updates in metadata_updates),
        "picasa_album": all(
            isinstance(asset, dict)
            and isinstance(asset.get("normalized_metadata"), dict)
            and asset["normalized_metadata"].get("albums") == ["Synthetic Picasa Album"]
            for asset in assets
        ),
        "live_photo_pair": all(
            isinstance(asset, dict) and asset.get("live_photo") is not None
            for asset in assets if asset.get("relative_path") in pair
        ) and directory_stacked == pair and not archive_stacked,
        "dry_run_mutations": _contained_mutations(directory) and _contained_mutations(archived),
    }
    failed = sorted(identifier for identifier, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("Picasa differential checks failed: " + ", ".join(failed))
    outcomes = {item["id"]: item["outcome"] for item in expectation["matrix"]}
    return {
        "schema": "picasa-differential-report-v1",
        "case_ids": case_ids,
        "oracle_version": directory.get("oracle", {}).get("version"),
        "checks": [
            {"id": identifier, "outcome": outcomes[identifier], "passed": True}
            for identifier in sorted(checks)
        ],
        "counts": {
            "planned_assets": len(planned),
            "directory_oracle_assets": len(observed[0]),
            "zip_oracle_assets": len(observed[1]),
            "sidecars": len(sidecars),
            "contained_oracle_mutations": 10,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expectation", type=Path)
    parser.add_argument("directory_observation", type=Path)
    parser.add_argument("zip_observation", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        report = compare(
            arguments.expectation,
            arguments.directory_observation,
            arguments.zip_observation,
        )
    except (DifferentialError, OSError, UnicodeError) as error:
        print(f"Picasa differential comparison failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
