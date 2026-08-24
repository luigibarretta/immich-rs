#!/usr/bin/env python3
"""Exercise two-server read-only migration planning against synthetic mocks."""

from __future__ import annotations

import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
MOCK = runpy.run_path(str(REPOSITORY_ROOT / "tests/oracle/mock_immich_server.py"))
MIGRATION = runpy.run_path(str(REPOSITORY_ROOT / "tests/oracle/mock_migration.py"))
SOURCE_KEY = MOCK["SYNTHETIC_MIGRATION_SOURCE_KEY"]
DESTINATION_KEY = MOCK["SYNTHETIC_MIGRATION_DESTINATION_KEY"]


def scenario(api_key: str, source: bool) -> dict[str, Any]:
    return MIGRATION["scenario"](MOCK["default_scenario"], api_key, source=source)


def environment(*, with_keys: bool = True) -> dict[str, str]:
    result = {
        "LANG": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    }
    if with_keys:
        result["IMMICH_RS_SOURCE_API_KEY"] = SOURCE_KEY
        result["IMMICH_RS_DESTINATION_API_KEY"] = DESTINATION_KEY
    if os.name == "nt":
        for name in ("COMSPEC", "SYSTEMROOT", "WINDIR"):
            value = os.environ.get(name)
            if value:
                result[name] = value
    return result


def invoke(
    binary: Path,
    arguments: list[str],
    *,
    success: bool = True,
    with_keys: bool = True,
) -> subprocess.CompletedProcess[bytes]:
    completed = subprocess.run(
        [str(binary), *arguments],
        check=False,
        capture_output=True,
        env=environment(with_keys=with_keys),
        timeout=20,
    )
    if success and (completed.returncode != 0 or completed.stderr):
        raise RuntimeError(
            f"migration CLI failed: exit={completed.returncode}, "
            f"stderr_bytes={len(completed.stderr)}"
        )
    return completed


def arguments(source_url: str, destination_url: str) -> list[str]:
    return [
        "plan",
        "migration",
        "immich",
        "--source-server",
        source_url,
        "--destination-server",
        destination_url,
        "--page-size",
        "1",
        "--max-assets",
        "3",
        "--max-albums",
        "1",
        "--max-album-memberships",
        "2",
        "--max-asset-bytes",
        "1024",
        "--max-total-bytes",
        "4096",
    ]


def plan_value(completed: subprocess.CompletedProcess[bytes]) -> dict[str, Any]:
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise RuntimeError("migration CLI did not emit a JSON object")
    return value


def exercise_plan(binary: Path, plan_path: Path) -> None:
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        command = arguments(source.url, destination.url)
        first_result = invoke(binary, command)
        second_result = invoke(binary, command)
        first = plan_value(first_result)
        second = plan_value(second_result)
        plan_path.write_bytes(first_result.stdout)
        source_snapshot = source.state.snapshot()
        destination_snapshot = destination.state.snapshot()
    summary = first.get("summary", {})
    roles = [asset.get("role", {}).get("kind") for asset in first.get("assets", [])]
    encoded = first_result.stdout + first_result.stderr
    if (
        first != second
        or first.get("schema_version") != 1
        or summary.get("assets") != 3
        or summary.get("metadata_updates") != 3
        or summary.get("album_creates") != 1
        or summary.get("max_mutations") != 8
        or sorted(roles) != ["live_photo_image", "live_photo_video", "standalone"]
        or len(first.get("albums", [])) != 1
        or source.url.encode() in encoded
        or destination.url.encode() in encoded
        or SOURCE_KEY.encode() in encoded
        or DESTINATION_KEY.encode() in encoded
    ):
        raise RuntimeError("migration plan is incomplete, unstable or endpoint-bearing")
    if (
        source_snapshot["committed_mutations"]
        or destination_snapshot["committed_mutations"]
        or any(request["mutating"] for request in source_snapshot["requests"])
        or any(request["mutating"] for request in destination_snapshot["requests"])
        or len(destination_snapshot["requests"]) != 4
    ):
        raise RuntimeError("migration planning reached a mutating capability")


def apply_arguments(
    plan_path: Path,
    checkpoint: Path,
    source_url: str,
    destination_url: str,
) -> list[str]:
    return [
        "apply",
        "migration",
        "immich",
        "--plan",
        str(plan_path),
        "--checkpoint",
        str(checkpoint),
        "--source-server",
        source_url,
        "--destination-server",
        destination_url,
        *arguments(source_url, destination_url)[7:],
    ]


def report_value(completed: subprocess.CompletedProcess[bytes]) -> dict[str, Any]:
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise RuntimeError("migration apply did not emit a JSON object")
    return value


def exercise_dry_run(binary: Path, plan_path: Path) -> None:
    completed = invoke(
        binary,
        [
            "apply",
            "migration",
            "immich",
            "--plan",
            str(plan_path),
            "--dry-run",
            *arguments("unused-source", "unused-destination")[7:],
        ],
        with_keys=False,
    )
    report = report_value(completed)
    if (
        not report.get("dry_run")
        or report.get("would_upload") != 3
        or report.get("would_update_metadata") != 3
        or report.get("would_create_albums") != 1
        or report.get("would_add_album_memberships") != 1
    ):
        raise RuntimeError("offline migration dry-run report is incomplete")


def assert_first_report(report: dict[str, Any]) -> None:
    expected = {
        "dry_run": False,
        "created": 3,
        "duplicate": 0,
        "metadata_updated": 3,
        "albums_created": 1,
        "albums_reused": 0,
        "album_memberships_updated": 1,
        "failed": 0,
        "indeterminate": 0,
        "cancelled": False,
    }
    if any(report.get(key) != value for key, value in expected.items()):
        raise RuntimeError("first migration apply report does not match effects")


def exercise_apply(binary: Path, root: Path) -> None:
    plan_path = root / "migration-plan.json"
    checkpoint = root / "migration-checkpoint.sqlite"
    duplicate_checkpoint = root / "migration-duplicate.sqlite"
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        plan = invoke(binary, arguments(source.url, destination.url))
        plan_path.write_bytes(plan.stdout)
        command = apply_arguments(plan_path, checkpoint, source.url, destination.url)
        first = report_value(invoke(binary, command))
        resumed = report_value(invoke(binary, command))
        duplicate = report_value(
            invoke(binary, apply_arguments(plan_path, duplicate_checkpoint, source.url, destination.url))
        )
        source_snapshot = source.state.snapshot()
        destination_snapshot = destination.state.snapshot()
    assert_first_report(first)
    if resumed.get("resumed_effects") != 8 or resumed.get("created") != 0:
        raise RuntimeError("migration checkpoint did not resume every durable effect")
    if (
        duplicate.get("duplicate") != 3
        or duplicate.get("metadata_updated") != 3
        or duplicate.get("albums_reused") != 1
        or duplicate.get("album_memberships_updated") != 1
    ):
        raise RuntimeError("fresh checkpoint did not converge through duplicate detection")
    if source_snapshot["committed_mutations"] or any(
        request["mutating"] for request in source_snapshot["requests"]
    ):
        raise RuntimeError("migration mutated the source server")
    if (
        destination_snapshot["asset_count"] != 3
        or destination_snapshot["metadata_count"] != 3
        or destination_snapshot["album_count"] != 1
        or destination_snapshot["album_memberships"] != 2
    ):
        raise RuntimeError("migration destination effects are incomplete")
    if list(root.glob(".immich-rs-migration-stage-*")):
        raise RuntimeError("migration left private staging behind")


def exercise_apply_source_drift(binary: Path, root: Path) -> None:
    plan_path = root / "drift-plan.json"
    checkpoint = root / "drift-checkpoint.sqlite"
    source_scenario = scenario(SOURCE_KEY, True)
    with (
        MOCK["running_mock"](source_scenario) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        plan_path.write_bytes(invoke(binary, arguments(source.url, destination.url)).stdout)
        for asset in source_scenario["archive_assets"]:
            asset["download_body"] = b"x" * len(asset["body"])
        completed = invoke(
            binary,
            apply_arguments(plan_path, checkpoint, source.url, destination.url),
            success=False,
        )
        destination_snapshot = destination.state.snapshot()
    if completed.returncode != 4 or completed.stdout:
        raise RuntimeError("migration apply did not fail closed on changed source bytes")
    if destination_snapshot["committed_mutations"]:
        raise RuntimeError("source drift reached a destination mutation")
    if list(root.glob(".immich-rs-migration-stage-*")):
        raise RuntimeError("failed migration left private staging behind")


def exercise_refusals(binary: Path) -> None:
    changed = scenario(SOURCE_KEY, True)
    changed["archive_assets"][0]["download_body"] = b"synthetic migration changed body\n"
    with (
        MOCK["running_mock"](changed) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        drift = invoke(binary, arguments(source.url, destination.url), success=False)
    if drift.returncode != 4 or drift.stdout:
        raise RuntimeError("changed source original did not fail closed")

    with MOCK["running_mock"](scenario(SOURCE_KEY, True)) as source:
        same = invoke(binary, arguments(source.url, source.url), success=False)
        requests = source.state.snapshot()["requests"]
    if same.returncode != 2 or same.stdout or requests:
        raise RuntimeError("same migration origin did not fail before authentication")

    refused_environment = environment()
    refused_environment["IMMICH_RS_DESTINATION_API_KEY"] = SOURCE_KEY
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        duplicate_key = subprocess.run(
            [str(binary), *arguments(source.url, destination.url)],
            check=False,
            capture_output=True,
            env=refused_environment,
            timeout=20,
        )
        source_requests = source.state.snapshot()["requests"]
        destination_requests = destination.state.snapshot()["requests"]
    if duplicate_key.returncode != 5 or duplicate_key.stdout or source_requests or destination_requests:
        raise RuntimeError("duplicate migration credentials did not fail before network access")


def main() -> int:
    if len(sys.argv) != 2:
        raise RuntimeError("usage: run_migration_mock.py <immich-rs-binary>")
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="immich-rs-migration-mock-") as directory:
        root = Path(directory).resolve()
        plan_path = root / "read-only-plan.json"
        exercise_plan(binary, plan_path)
        exercise_dry_run(binary, plan_path)
        exercise_apply(binary, root)
        exercise_apply_source_drift(binary, root)
        exercise_refusals(binary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
