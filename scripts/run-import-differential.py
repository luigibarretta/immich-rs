#!/usr/bin/env python3
"""Compare source-aware Apple or Picasa imports against pinned immich-go."""

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
MATERIALIZER = runpy.run_path(str(ROOT / "scripts/materialize-fixture.py"))
ORACLE = runpy.run_path(str(ROOT / "scripts/run-oracle.py"))
CAPTURE = runpy.run_path(str(ROOT / "scripts/bounded-process.py"))
NORMALIZE = runpy.run_path(str(ROOT / "scripts/oracle_capture.py"))
API_KEY = MOCK["SYNTHETIC_API_KEY"]
COMPATIBILITY_ROOT = (ROOT / "tests/oracle/compatibility").resolve()
FIXTURE_ROOT = (ROOT / "tests/fixtures").resolve()


class DifferentialError(ValueError):
    """The source-aware import differential failed its declared matrix."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def environment(home: Path, *, rust: bool) -> dict[str, str]:
    home.mkdir(exist_ok=True)
    result = {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "NO_COLOR": "1", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC",
    }
    if rust:
        result["IMMICH_RS_API_KEY"] = API_KEY
    return result


def load_expectation(path: Path) -> tuple[dict[str, Any], Path, dict[str, str]]:
    resolved = path.resolve()
    if COMPATIBILITY_ROOT not in resolved.parents:
        raise DifferentialError("expectation escaped the compatibility root")
    try:
        value = json.loads(resolved.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DifferentialError("cannot read import expectation") from error
    adapter = value.get("adapter") if isinstance(value, dict) else None
    case_id = value.get("case_id") if isinstance(value, dict) else None
    matrix = value.get("matrix") if isinstance(value, dict) else None
    if (
        value.get("schema") != "source-import-differential-expectation-v1"
        or adapter not in {"apple-photos", "picasa"}
        or case_id != ("apple-import-v1" if adapter == "apple-photos" else "picasa-import-v1")
        or not isinstance(matrix, list)
    ):
        raise DifferentialError("import expectation identity is invalid")
    fixture_value = value.get("fixture")
    if not isinstance(fixture_value, str):
        raise DifferentialError("import fixture path is invalid")
    fixture = (resolved.parent / fixture_value).resolve()
    if FIXTURE_ROOT not in fixture.parents or fixture.name != "manifest.json":
        raise DifferentialError("import fixture escaped its root")
    outcomes = {
        item.get("id"): item.get("outcome") for item in matrix if isinstance(item, dict)
    }
    expected = {
        "oracle_identity", "asset_uploads", "live_photo", "xmp_sidecar",
        "checkpoint_resume", "job_mutations",
    }
    expected |= (
        {"owned_album"}
        if adapter == "apple-photos"
        else {"filename_date", "picasa_caption", "picasa_album"}
    )
    if set(outcomes) != expected or any(not isinstance(item, str) for item in outcomes.values()):
        raise DifferentialError("import matrix is incomplete")
    return value, fixture, outcomes


def source_arguments(adapter: str) -> list[str]:
    if adapter == "apple-photos":
        return ["--album-mode", "folder"]
    return ["--album-mode", "none", "--picasa-albums", "--filename-date"]


def run_rs(
    binary: Path, adapter: str, fixture: Path, workspace: Path,
) -> dict[str, Any]:
    source = workspace / "rs-source"
    MATERIALIZER["materialize"](fixture, source, None)
    home = workspace / "rs-home"
    plan_path = workspace / "rs-plan.json"
    checkpoint = workspace / "rs-checkpoint.sqlite"
    with MOCK["running_mock"]() as server:
        planned = CAPTURE["run_command"](
            [
                str(binary), "plan", "upload", adapter, "--server", server.url,
                *source_arguments(adapter), str(source),
            ],
            workspace, environment(home, rust=True), 30,
        )
        if planned.returncode != 0 or planned.stderr:
            raise DifferentialError("immich-rs import planning failed")
        plan_path.write_text(planned.stdout, encoding="utf-8")
        command = [
            str(binary), "apply", "upload", "--plan", str(plan_path),
            "--source", str(source), "--checkpoint", str(checkpoint),
            "--server", server.url, *source_arguments(adapter),
        ]
        applied = CAPTURE["run_command"](command, workspace, environment(home, rust=True), 30)
        resumed = CAPTURE["run_command"](command, workspace, environment(home, rust=True), 30)
        state = server.state.snapshot()
    if applied.returncode != 0 or resumed.returncode != 0:
        raise DifferentialError("immich-rs import apply failed")
    return {
        "plan": json.loads(planned.stdout), "report": json.loads(applied.stdout),
        "resume": json.loads(resumed.stdout), "destination": state,
    }


def oracle_arguments(adapter: str, server_url: str, source: Path) -> list[str]:
    command = "from-icloud" if adapter == "apple-photos" else "from-picasa"
    specific = (
        ["--folder-as-album", "FOLDER"]
        if adapter == "apple-photos"
        else ["--album-picasa=true", "--folder-as-album", "NONE", "--date-from-name=true"]
    )
    return [
        "upload", command, "--server", server_url, "--api-key", API_KEY,
        "--device-uuid", f"synthetic-{adapter}-import", "--no-ui", "--log-level", "INFO",
        "--pause-immich-jobs=false", "--concurrent-tasks", "1", "--on-errors", "stop",
        *specific, str(source),
    ]


def normalized_process(
    result: Any, home: Path, workspace: Path, source: Path, server_url: str,
) -> dict[str, Any]:
    def clean(value: str) -> str:
        return value.replace(API_KEY, "<API_KEY>").replace(server_url, "<MOCK_SERVER>")

    logs = NORMALIZE["captured_logs"](home, source, workspace, server_url)
    for log in logs:
        log["lines"] = [clean(line) for line in log["lines"]]
    return {
        "exit_code": result.returncode,
        "stdout": NORMALIZE["normalize_text"](clean(result.stdout), source, workspace, ""),
        "stderr": NORMALIZE["normalize_text"](clean(result.stderr), source, workspace, ""),
        "logs": logs,
    }


def run_oracle(
    binary: Path, adapter: str, fixture: Path, workspace: Path,
) -> dict[str, Any]:
    source = workspace / "oracle-source"
    MATERIALIZER["materialize"](fixture, source, None)
    home = workspace / "oracle-home"
    with MOCK["running_mock"]() as server:
        result = CAPTURE["run_command"](
            [str(binary), *oracle_arguments(adapter, server.url, source)],
            workspace, environment(home, rust=False), 60,
        )
        process = normalized_process(result, home, workspace, source, server.url)
        state = server.state.snapshot()
    if result.returncode != 0:
        raise DifferentialError("immich-go import oracle failed")
    return {"process": process, "destination": state}


def job_mutations(state: dict[str, Any]) -> Counter[str]:
    return Counter(
        request["path"] for request in state["requests"]
        if request.get("mutating") is True and request["path"].startswith("/api/jobs/")
    )


def compare(
    expectation: dict[str, Any], outcomes: dict[str, str], fixture: Path,
    rs: dict[str, Any], oracle: dict[str, Any], identity: dict[str, str],
) -> dict[str, Any]:
    adapter = expectation["adapter"]
    rs_state = rs["destination"]
    oracle_state = oracle["destination"]
    expected_assets = 5 if adapter == "apple-photos" else 4
    expected_resume = 7 if adapter == "apple-photos" else 8
    checks = {
        "oracle_identity": identity.get("version") == "0.32.0",
        "asset_uploads": rs_state["asset_count"] == oracle_state["asset_count"] == expected_assets,
        "live_photo": rs["plan"]["summary"].get("live_photo_pairs") == 1,
        "xmp_sidecar": rs["plan"]["summary"].get("xmp_sidecars") == 1,
        "checkpoint_resume": rs["resume"].get("resumed_effects") == expected_resume,
        "job_mutations": len(job_mutations(oracle_state)) == 5
        and not job_mutations(rs_state),
    }
    if adapter == "apple-photos":
        checks["owned_album"] = all(
            state["album_count"] == 1 and state["album_memberships"] == 5
            for state in (rs_state, oracle_state)
        )
    else:
        checks.update({
            "filename_date": rs_state["metadata_count"] >= 1 and oracle_state["metadata_count"] == 1,
            "picasa_caption": rs_state["metadata_count"] == 2,
            "picasa_album": rs_state["album_count"] == 1
            and rs_state["album_memberships"] == 4 and oracle_state["album_count"] == 0,
        })
    failed = sorted(identifier for identifier, passed in checks.items() if not passed)
    if failed:
        raise DifferentialError("source import checks failed: " + ", ".join(failed))
    return {
        "schema": "source-import-differential-report-v1",
        "case_id": expectation["case_id"], "adapter": adapter,
        "fixture": {"sha256": sha256(fixture), "kind": "synthetic", "license": "CC0-1.0"},
        "oracle": identity,
        "checks": [
            {"id": identifier, "outcome": outcomes[identifier], "passed": True}
            for identifier in sorted(checks)
        ],
        "observed": {
            "immich_rs_assets": rs_state["asset_count"],
            "immich_go_assets": oracle_state["asset_count"],
            "immich_rs_metadata_updates": rs_state["metadata_count"],
            "immich_go_metadata_updates": oracle_state["metadata_count"],
            "immich_rs_albums": rs_state["album_count"],
            "immich_go_albums": oracle_state["album_count"],
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
        expectation, fixture, outcomes = load_expectation(options.expectation)
        identity = ORACLE["verify_oracle"](options.oracle.resolve(), ORACLE["load_baseline"]())
        with tempfile.TemporaryDirectory(prefix="immich-rs-import-differential-") as value:
            workspace = Path(value).resolve()
            report = compare(
                expectation, outcomes, fixture,
                run_rs(options.immich_rs.resolve(), expectation["adapter"], fixture, workspace),
                run_oracle(options.oracle.resolve(), expectation["adapter"], fixture, workspace),
                identity,
            )
        encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        if API_KEY in encoded or "http://127.0.0.1" in encoded:
            raise DifferentialError("import differential leaked a secret or origin")
        if options.output:
            options.output.write_text(encoded, encoding="utf-8")
        else:
            sys.stdout.write(encoded)
    except (DifferentialError, OSError, UnicodeError, ValueError) as error:
        print(f"source import differential failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
