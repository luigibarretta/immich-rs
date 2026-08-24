#!/usr/bin/env python3
"""Exercise two-server read-only migration planning against synthetic mocks."""

from __future__ import annotations

import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
MOCK = runpy.run_path(str(REPOSITORY_ROOT / "tests/oracle/mock_immich_server.py"))
SOURCE_KEY = MOCK["SYNTHETIC_MIGRATION_SOURCE_KEY"]
DESTINATION_KEY = MOCK["SYNTHETIC_MIGRATION_DESTINATION_KEY"]
ALBUM_ID = "00000000-0000-4000-8000-000000000204"
IMAGE_ID = "00000000-0000-4000-8000-000000000201"
VIDEO_ID = "00000000-0000-4000-8000-000000000202"
STANDALONE_ID = "00000000-0000-4000-8000-000000000203"
ASSETS = [
    {
        "id": IMAGE_ID,
        "filename": "synthetic-live.jpg",
        "body": b"synthetic migration live image\n",
        "type": "IMAGE",
        "visibility": "timeline",
        "live_photo_video_id": VIDEO_ID,
        "album_ids": [ALBUM_ID],
        "date_time_original": "2024-02-03T04:05:06Z",
        "description": "synthetic migration description",
        "latitude": 12.5,
        "longitude": -45.25,
    },
    {
        "id": VIDEO_ID,
        "filename": "synthetic-live.mov",
        "body": b"synthetic migration live motion\n",
        "type": "VIDEO",
        "visibility": "linked",
    },
    {
        "id": STANDALONE_ID,
        "filename": "synthetic-standalone.jpg",
        "body": b"synthetic migration standalone\n",
        "type": "IMAGE",
        "visibility": "timeline",
        "album_ids": [ALBUM_ID],
    },
]


def scenario(api_key: str, source: bool) -> dict[str, Any]:
    selected = MOCK["default_scenario"]()
    selected["api_key"] = api_key
    if source:
        selected["archive_assets"] = [dict(asset) for asset in ASSETS]
        selected["archive_albums"] = [
            {
                "id": ALBUM_ID,
                "name": "Synthetic Migration Album",
                "asset_ids": [IMAGE_ID, STANDALONE_ID],
            }
        ]
    return selected


def environment() -> dict[str, str]:
    result = {
        "LANG": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "IMMICH_RS_SOURCE_API_KEY": SOURCE_KEY,
        "IMMICH_RS_DESTINATION_API_KEY": DESTINATION_KEY,
    }
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
) -> subprocess.CompletedProcess[bytes]:
    completed = subprocess.run(
        [str(binary), *arguments],
        check=False,
        capture_output=True,
        env=environment(),
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


def exercise_plan(binary: Path) -> None:
    with (
        MOCK["running_mock"](scenario(SOURCE_KEY, True)) as source,
        MOCK["running_mock"](scenario(DESTINATION_KEY, False)) as destination,
    ):
        command = arguments(source.url, destination.url)
        first_result = invoke(binary, command)
        second_result = invoke(binary, command)
        first = plan_value(first_result)
        second = plan_value(second_result)
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
    exercise_plan(binary)
    exercise_refusals(binary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
