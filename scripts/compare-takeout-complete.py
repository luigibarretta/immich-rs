#!/usr/bin/env python3
"""Compare complete Takeout v2 facts with a normalized immich-go observation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EXPECTATION_ROOT = (REPOSITORY_ROOT / "tests" / "oracle" / "compatibility").resolve()
PLAN_ROOT = (REPOSITORY_ROOT / "tests" / "fixtures" / "v2").resolve()
EXPECTED_CHECKS = {
    "album_membership", "content_alias", "dry_run_mutations",
    "normalized_description_location", "split_zip_layout",
    "supplemental_filename", "title_metadata", "unicode_nfc", "utc_timestamp",
}
FILE_PATTERN = re.compile(
    r"\bfile=(?:takeout-\d+:|fixture:)(.+?)"
    r"(?:\s+(?:type|title|date|reason|file\.).*)?$"
)


class DifferentialError(ValueError):
    """The complete Takeout observation does not satisfy its declared matrix."""


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
    expectation = _json_object(resolved, "Takeout v2 expectation")
    if expectation.get("schema") != "takeout-differential-expectation-v2":
        raise DifferentialError("unsupported Takeout v2 expectation schema")
    raw_plan = expectation.get("expected_plan")
    if not isinstance(raw_plan, str):
        raise DifferentialError("expected_plan must be a relative string")
    plan_path = (resolved.parent / raw_plan).resolve()
    if PLAN_ROOT not in plan_path.parents or plan_path.name != "expected-plan.json":
        raise DifferentialError("expected plan must be a v2 synthetic golden")
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
        raise DifferentialError("matrix must declare every Takeout v2 check once")
    return expectation, plan_path


def _process_lines(observation: dict[str, Any]) -> list[str]:
    process = observation.get("process")
    if not isinstance(process, dict):
        raise DifferentialError("observation process is malformed")
    stdout = process.get("stdout", [])
    logs = process.get("logs", [])
    if not isinstance(stdout, list) or not isinstance(logs, list):
        raise DifferentialError("observation text capture is malformed")
    combined = [line for line in stdout if isinstance(line, str)]
    for log in logs:
        if not isinstance(log, dict) or not isinstance(log.get("lines"), list):
            raise DifferentialError("observation log is malformed")
        combined.extend(line for line in log["lines"] if isinstance(line, str))
    return combined


def _paths(lines: list[str], marker: str) -> set[str]:
    result = set()
    for line in lines:
        if marker in line and (match := FILE_PATTERN.search(line)):
            result.add(match.group(1))
    return result


def _plan_facts(plan: dict[str, Any]) -> dict[str, Any]:
    assets = plan.get("assets")
    if plan.get("schema_version") != 2 or not isinstance(assets, list):
        raise DifferentialError("golden is not a normalized-plan-v2")
    paths = set()
    sidecars = set()
    metadata: dict[str, dict[str, Any]] = {}
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("relative_path"), str):
            raise DifferentialError("golden asset is malformed")
        path = asset["relative_path"]
        paths.add(path)
        normalized = asset.get("normalized_metadata")
        if not isinstance(normalized, dict):
            raise DifferentialError("golden asset lacks normalized metadata")
        metadata[path] = normalized
        candidates = asset.get("metadata")
        if not isinstance(candidates, list):
            raise DifferentialError("golden metadata candidates are malformed")
        for candidate in candidates:
            if not isinstance(candidate, dict) or not isinstance(
                candidate.get("relative_path"), str
            ):
                raise DifferentialError("golden metadata candidate is malformed")
            sidecars.add(candidate["relative_path"])
    alias_paths = set()
    warnings = plan.get("warnings")
    if not isinstance(warnings, list):
        raise DifferentialError("golden warnings are malformed")
    for warning in warnings:
        if not isinstance(warning, dict) or warning.get("rule_id") != (
            "META_GOOGLE_TAKEOUT_CONTENT_ALIAS_V1"
        ):
            continue
        raw_paths = warning.get("paths")
        if isinstance(raw_paths, list):
            alias_paths.update(path for path in raw_paths if isinstance(path, str))
    return {
        "paths": paths, "sidecars": sidecars,
        "metadata": metadata, "alias_paths": alias_paths,
    }


def _mutation_check(commits: list[object]) -> bool:
    return bool(commits) and all(
        isinstance(commit, dict)
        and commit.get("method") == "PUT"
        and isinstance(commit.get("path"), str)
        and commit["path"].startswith("/api/jobs/")
        for commit in commits
    ) and not any(
        isinstance(commit, dict) and commit.get("path") == "/api/assets"
        for commit in commits
    )


def compare(expectation_path: Path, observation_path: Path) -> dict[str, Any]:
    """Compare normalized complete Takeout facts and emit a stable report."""
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
    facts = _plan_facts(plan)
    lines = _process_lines(observation)
    discovered = _paths(lines, "discovered image file=")
    uploaded = _paths(lines, "uploaded successfully file=")
    duplicates = _paths(lines, "discarded local duplicate file=")
    discovered_sidecars = _paths(lines, "discovered sidecar file=")
    metadata_updated = _paths(lines, "metadata updated file=")
    alpha = "Takeout/Google Photos/Photos from 2024/alpha.png"
    beta = "Takeout/Google Photos/Photos from 2024/beta.png"
    cafe = "Takeout/Google Photos/Photos from 2024/café.png"
    album_alias = "Takeout/Google Photos/Synthetic Album/alpha.png"
    supplemental = beta + ".supplemental-metadata.json"
    physical = facts["paths"] | {album_alias}
    commits = observable.get("committed_mutations")
    if not isinstance(commits, list):
        raise DifferentialError("oracle mutation set is malformed")
    normalized = facts["metadata"]
    checks = {
        "split_zip_layout": fixture.get("archive_view") == "split"
        and discovered == physical
        and any("takeout-001:" in line for line in lines)
        and any("takeout-002:" in line for line in lines),
        "title_metadata": discovered_sidecars == facts["sidecars"]
        and {alpha, cafe} <= metadata_updated,
        "supplemental_filename": any(
            supplemental in line and "type=album metadata" in line for line in lines
        ) and beta not in uploaded,
        "content_alias": facts["alias_paths"] == {alpha, album_alias}
        and duplicates == {album_alias}
        and alpha in uploaded,
        "album_membership": normalized[alpha].get("albums") == ["Synthetic Album"]
        and any("added to album" in line and ":       3" in line for line in lines),
        "normalized_description_location": all(
            data.get("description") for data in normalized.values()
        )
        and normalized[alpha].get("location")
        == {"latitude": "3.5", "longitude": "-4.25"}
        and alpha in metadata_updated,
        "utc_timestamp": [
            normalized[path].get("taken_at_utc") for path in (alpha, beta, cafe)
        ]
        == [
            "2024-01-01T00:00:00Z",
            "2024-01-02T00:00:00Z",
            "2024-01-03T00:00:00Z",
        ]
        and sum("date=<TIMESTAMP>" in line for line in lines) >= 3,
        "unicode_nfc": cafe in discovered and cafe in uploaded,
        "dry_run_mutations": _mutation_check(commits),
    }
    failed = sorted(identifier for identifier, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("Takeout v2 differential checks failed: " + ", ".join(failed))
    outcomes = {item["id"]: item["outcome"] for item in expectation["matrix"]}
    oracle = observation.get("oracle")
    return {
        "schema": "takeout-differential-report-v2",
        "case_id": expectation["case_id"],
        "oracle_version": oracle.get("version") if isinstance(oracle, dict) else None,
        "checks": [
            {"id": identifier, "outcome": outcomes[identifier], "passed": True}
            for identifier in sorted(checks)
        ],
        "counts": {
            "planned_assets": len(facts["paths"]),
            "oracle_physical_assets": len(discovered),
            "oracle_uploaded_assets": len(uploaded),
            "sidecars": len(facts["sidecars"]),
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
        print(f"Takeout v2 differential comparison failed: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
