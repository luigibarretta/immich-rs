"""Synthetic server-bound Takeout planning checks for the Phase-2 mock gate."""

from __future__ import annotations

import json
from pathlib import Path
import zipfile


def exercise_takeout_plan(binary, workspace: Path, mock, invoke, invoke_process) -> None:
    source = workspace / "takeout-source"
    year = source / "Takeout" / "Google Photos" / "Photos from 2024"
    album = source / "Takeout" / "Google Photos" / "Synthetic Album"
    year.mkdir(parents=True)
    album.mkdir(parents=True)
    media = b"synthetic Takeout image\n"
    sidecar = json.dumps(
        {
            "title": "takeout.jpg",
            "description": "synthetic description",
            "photoTakenTime": {"timestamp": "1704067200"},
        },
        separators=(",", ":"),
    ).encode()
    (year / "takeout.jpg").write_bytes(media)
    (year / "takeout.jpg.json").write_bytes(sidecar)
    (album / "takeout.jpg").write_bytes(media)
    (album / "takeout.jpg.json").write_bytes(sidecar)
    (album / "metadata.json").write_text(
        '{"title":"Synthetic Album"}', encoding="utf-8"
    )
    archive = workspace / "takeout.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
        for path in sorted(source.rglob("*")):
            if path.is_file():
                output.write(path, path.relative_to(source).as_posix())
    with mock["running_mock"]() as server:
        arguments = [
            "plan",
            "upload",
            "google-takeout",
            "--server",
            server.url,
            "--label",
            "synthetic-takeout",
            "--buffer-bytes",
            "4096",
        ]
        directory_plan = invoke(binary, [*arguments, str(source)], with_key=True)
        archive_plan = invoke(binary, [*arguments, str(archive)], with_key=True)
        plan_path = workspace / "takeout-plan.json"
        plan_path.write_text(json.dumps(directory_plan), encoding="utf-8")
        requests_before_apply = len(server.state.snapshot()["requests"])
        checkpoint = workspace / "takeout.sqlite"
        dry_directory = invoke(
            binary,
            [
                "apply",
                "upload",
                "--dry-run",
                "--plan",
                str(plan_path),
                "--buffer-bytes",
                "4096",
                "--source",
                str(source),
                "--checkpoint",
                str(checkpoint),
            ],
            with_key=False,
        )
        dry_archive = invoke(
            binary,
            [
                "apply",
                "upload",
                "--dry-run",
                "--plan",
                str(plan_path),
                "--buffer-bytes",
                "4096",
                "--input",
                str(archive),
                "--checkpoint",
                str(checkpoint),
            ],
            with_key=False,
        )
        refused = invoke_process(
            binary,
            [
                "apply",
                "upload",
                "--server",
                server.url,
                "--plan",
                str(plan_path),
                "--source",
                str(source),
                "--checkpoint",
                str(checkpoint),
            ],
            with_key=False,
        )
        snapshot = server.state.snapshot()
    if directory_plan != archive_plan:
        raise RuntimeError("directory and ZIP Takeout upload plans differ")
    summary = directory_plan.get("summary", {})
    if (
        directory_plan.get("schema_version") != 2
        or summary.get("operations") != 1
        or summary.get("max_mutations") != 4
        or server.url in json.dumps(directory_plan)
        or snapshot["committed_mutations"]
        or dry_directory != dry_archive
        or dry_directory.get("would_upload") != 1
        or dry_directory.get("would_update_metadata") != 1
        or dry_directory.get("would_create_albums") != 1
        or dry_directory.get("would_add_album_memberships") != 1
        or checkpoint.exists()
        or refused.returncode != 2
        or refused.stdout
        or len(snapshot["requests"]) != requests_before_apply
    ):
        raise RuntimeError("Takeout upload plan is incomplete, unsafe or endpoint-bearing")
