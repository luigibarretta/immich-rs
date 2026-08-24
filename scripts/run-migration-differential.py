#!/usr/bin/env python3
"""Run a bounded two-server differential against pinned immich-go."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
MOCK = runpy.run_path(str(ROOT / "tests/oracle/mock_immich_server.py"))
MIGRATION = runpy.run_path(str(ROOT / "tests/oracle/mock_migration.py"))
ORACLE = runpy.run_path(str(ROOT / "scripts/run-oracle.py"))
CAPTURE = runpy.run_path(str(ROOT / "scripts/bounded-process.py"))
NORMALIZE = runpy.run_path(str(ROOT / "scripts/oracle_capture.py"))
SOURCE_KEY = MOCK["SYNTHETIC_MIGRATION_SOURCE_KEY"]
DESTINATION_KEY = MOCK["SYNTHETIC_MIGRATION_DESTINATION_KEY"]
FIXTURE = ROOT / "tests/oracle/server-fixtures/immich-migration-v1.json"
EXPECTED_CHECKS = {
    "checkpoint_resume", "description_location", "job_mutations", "live_photo_motion",
    "oracle_identity", "owned_album", "source_immutability", "timeline_originals",
}


class DifferentialError(ValueError):
    """The migration differential did not satisfy its declared matrix."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def scenario(key: str, *, source: bool) -> dict[str, Any]:
    return MIGRATION["scenario"](MOCK["default_scenario"], key, source=source)


def environment(home: Path, *, keys: bool) -> dict[str, str]:
    result = {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "NO_COLOR": "1", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC",
    }
    if keys:
        result["IMMICH_RS_SOURCE_API_KEY"] = SOURCE_KEY
        result["IMMICH_RS_DESTINATION_API_KEY"] = DESTINATION_KEY
    return result


def resource_arguments() -> list[str]:
    return [
        "--page-size", "1", "--max-assets", "3", "--max-albums", "1",
        "--max-album-memberships", "2", "--max-asset-bytes", "1024",
        "--max-total-bytes", "4096", "--concurrency", "1",
    ]


def run_rs(binary: Path, workspace: Path) -> dict[str, Any]:
    home = workspace / "rs-home"
    home.mkdir()
    plan_path = workspace / "migration-plan.json"
    checkpoint = workspace / "migration-checkpoint.sqlite"
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, source=True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, source=False)) as destination,
    ):
        common = ["--source-server", source.url, "--destination-server", destination.url]
        plan = CAPTURE["run_command"](
            [str(binary), "plan", "migration", "immich", *common, *resource_arguments()],
            workspace, environment(home, keys=True), 30,
        )
        if plan.returncode != 0 or plan.stderr:
            raise DifferentialError("immich-rs migration planning failed")
        plan_path.write_text(plan.stdout, encoding="utf-8")
        command = [
            str(binary), "apply", "migration", "immich", "--plan", str(plan_path),
            "--checkpoint", str(checkpoint), *common, *resource_arguments(),
        ]
        applied = CAPTURE["run_command"](command, workspace, environment(home, keys=True), 30)
        resumed = CAPTURE["run_command"](command, workspace, environment(home, keys=True), 30)
        source_state = source.state.snapshot()
        destination_state = destination.state.snapshot()
    if applied.returncode != 0 or resumed.returncode != 0:
        raise DifferentialError("immich-rs migration apply failed")
    first_report = json.loads(applied.stdout)
    resume_report = json.loads(resumed.stdout)
    return {
        "source": source_state,
        "destination": destination_state,
        "report": first_report,
        "resume": resume_report,
        "plan_sha256": sha256(plan_path),
    }


def oracle_command(executable: Path, source_url: str, destination_url: str) -> list[str]:
    return [
        str(executable), "upload", "from-immich", "--from-server", source_url,
        "--from-api-key", SOURCE_KEY, "--server", destination_url, "--api-key",
        DESTINATION_KEY, "--from-device-uuid", "synthetic-migration-source",
        "--device-uuid", "synthetic-migration-destination",
        "--from-pause-immich-jobs=false", "--pause-immich-jobs=false", "--no-ui",
        "--log-level", "INFO", "--concurrent-tasks", "1", "--on-errors", "stop",
    ]


def normalized_process(
    result: Any, home: Path, workspace: Path, source_url: str, destination_url: str,
) -> dict[str, Any]:
    def clean(value: str) -> str:
        return value.replace(SOURCE_KEY, "<SOURCE_API_KEY>").replace(
            DESTINATION_KEY, "<DESTINATION_API_KEY>"
        ).replace(source_url, "<SOURCE_SERVER>").replace(destination_url, "<DESTINATION_SERVER>")

    logs = NORMALIZE["captured_logs"](home, workspace, workspace, source_url)
    for log in logs:
        log["lines"] = [clean(line) for line in log["lines"]]
    return {
        "exit_code": result.returncode,
        "stdout": NORMALIZE["normalize_text"](clean(result.stdout), workspace, workspace, ""),
        "stderr": NORMALIZE["normalize_text"](clean(result.stderr), workspace, workspace, ""),
        "logs": logs,
    }


def run_oracle(executable: Path, workspace: Path) -> dict[str, Any]:
    verified = ORACLE["verify_oracle"](executable, ORACLE["load_baseline"]())
    home = workspace / "oracle-home"
    home.mkdir()
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, source=True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, source=False)) as destination,
    ):
        result = CAPTURE["run_command"](
            oracle_command(executable, source.url, destination.url),
            workspace, environment(home, keys=False), 60,
        )
        process = normalized_process(result, home, workspace, source.url, destination.url)
        source_state = source.state.snapshot()
        destination_state = destination.state.snapshot()
    if result.returncode != 0:
        raise DifferentialError("immich-go migration oracle failed")
    return {
        "identity": verified, "process": process,
        "source": source_state, "destination": destination_state,
    }


def mutating_paths(state: dict[str, Any]) -> Counter[str]:
    return Counter(
        request["path"] for request in state["requests"] if request.get("mutating") is True
    )


def metadata_has_description_location(state: dict[str, Any]) -> bool:
    return any(
        value.get("description") == "synthetic migration description"
        and value.get("latitude") == 12.5 and value.get("longitude") == -45.25
        for value in state.get("metadata", {}).values() if isinstance(value, dict)
    )


def load_expectation(path: Path) -> dict[str, str]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DifferentialError("cannot read migration expectation") from error
    matrix = value.get("matrix") if isinstance(value, dict) else None
    if (
        value.get("schema") != "immich-migration-differential-expectation-v1"
        or value.get("case_id") != "immich-migration-v1"
        or value.get("fixture_id") != "synthetic-immich-migration"
        or not isinstance(matrix, list)
    ):
        raise DifferentialError("migration expectation is invalid")
    outcomes = {
        item.get("id"): item.get("outcome") for item in matrix if isinstance(item, dict)
    }
    if set(outcomes) != EXPECTED_CHECKS or any(not isinstance(item, str) for item in outcomes.values()):
        raise DifferentialError("migration expectation matrix is incomplete")
    return outcomes


def compare(outcomes: dict[str, str], rs: dict[str, Any], oracle: dict[str, Any]) -> dict[str, Any]:
    rs_destination = rs["destination"]
    oracle_destination = oracle["destination"]
    job_paths = Counter({
        path: count
        for path, count in mutating_paths(oracle_destination).items()
        if path.startswith("/api/jobs/")
    })
    checks = {
        "oracle_identity": oracle["identity"].get("version") == "0.32.0",
        "source_immutability": not rs["source"]["committed_mutations"]
        and not oracle["source"]["committed_mutations"],
        "timeline_originals": rs_destination["asset_count"] == 3
        and oracle_destination["asset_count"] == 2,
        "description_location": metadata_has_description_location(rs_destination)
        and metadata_has_description_location(oracle_destination),
        "owned_album": all(
            state["album_count"] == 1 and state["album_memberships"] == 2
            for state in (rs_destination, oracle_destination)
        ),
        "live_photo_motion": rs_destination["asset_count"] - oracle_destination["asset_count"] == 1,
        "checkpoint_resume": rs["resume"].get("resumed_effects") == 8,
        "job_mutations": len(job_paths) == 5
        and all(count == 1 and path.startswith("/api/jobs/") for path, count in job_paths.items()),
    }
    failed = sorted(identifier for identifier, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("migration differential checks failed: " + ", ".join(failed))
    return {
        "schema": "immich-migration-differential-report-v1",
        "case_id": "immich-migration-v1",
        "fixture": {"id": "synthetic-immich-migration", "sha256": sha256(FIXTURE)},
        "oracle": oracle["identity"],
        "checks": [
            {"id": identifier, "outcome": outcomes[identifier], "passed": True}
            for identifier in sorted(checks)
        ],
        "observed": {
            "immich_rs_assets": rs_destination["asset_count"],
            "immich_go_assets": oracle_destination["asset_count"],
            "immich_rs_resumed_effects": rs["resume"]["resumed_effects"],
            "owned_album_memberships": 2,
            "contained_oracle_job_mutations": 5,
        },
        "oracle_process": oracle["process"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expectation", type=Path)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--immich-rs", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    options = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix="immich-rs-migration-differential-") as value:
            workspace = Path(value).resolve()
            report = compare(
                load_expectation(options.expectation),
                run_rs(options.immich_rs.resolve(), workspace),
                run_oracle(options.oracle.resolve(), workspace),
            )
        encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        if any(secret in encoded for secret in (SOURCE_KEY, DESTINATION_KEY, "http://127.0.0.1")):
            raise DifferentialError("migration differential output contains a secret or origin")
        if options.output:
            options.output.write_text(encoded, encoding="utf-8")
        else:
            sys.stdout.write(encoded)
    except (DifferentialError, OSError, ValueError) as error:
        print(f"migration differential failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
