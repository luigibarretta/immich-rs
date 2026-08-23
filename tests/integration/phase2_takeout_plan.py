"""Synthetic server-bound Takeout planning checks for the Phase-2 mock gate."""

from __future__ import annotations

import json
import hashlib
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
        apply_arguments = [
            "apply",
            "upload",
            "--server",
            server.url,
            "--plan",
            str(plan_path),
            "--buffer-bytes",
            "4096",
            "--source",
            str(source),
            "--checkpoint",
            str(checkpoint),
        ]
        first = invoke(binary, apply_arguments, with_key=True)
        resumed = invoke(binary, apply_arguments, with_key=True)
        archive_checkpoint = workspace / "takeout-archive.sqlite"
        archive_apply = list(apply_arguments)
        archive_apply[archive_apply.index("--source")] = "--input"
        archive_apply[archive_apply.index(str(source))] = str(archive)
        archive_apply[archive_apply.index(str(checkpoint))] = str(archive_checkpoint)
        duplicate = invoke(binary, archive_apply, with_key=True)
        requests_before_refusal = len(server.state.snapshot()["requests"])
        plan_digest = hashlib.sha256(
            json.dumps(directory_plan, separators=(",", ":")).encode()
        ).hexdigest()
        refused = invoke_process(
            binary,
            [
                *apply_arguments,
                "--authorize-production-read",
                "--authorize-production-write",
                "--confirm-plan-sha256",
                plan_digest,
                "--expected-operations",
                "4",
                "--backup-reference",
                "synthetic-backup",
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
        or dry_directory != dry_archive
        or dry_directory.get("would_upload") != 1
        or dry_directory.get("would_update_metadata") != 1
        or dry_directory.get("would_create_albums") != 1
        or dry_directory.get("would_add_album_memberships") != 1
        or not checkpoint.exists()
        or first.get("created") != 1
        or first.get("metadata_updated") != 1
        or first.get("albums_created") != 1
        or first.get("album_memberships_updated") != 1
        or resumed.get("resumed_effects") != 4
        or duplicate.get("duplicate") != 1
        or duplicate.get("metadata_updated") != 1
        or duplicate.get("albums_reused") != 1
        or duplicate.get("album_memberships_updated") != 1
        or snapshot.get("asset_count") != 1
        or snapshot.get("metadata_count") != 1
        or snapshot.get("album_count") != 1
        or snapshot.get("album_memberships") != 1
        or refused.returncode != 2
        or refused.stdout
        or requests_before_apply != 4
        or len(snapshot["requests"]) != requests_before_refusal
    ):
        raise RuntimeError("Takeout upload plan is incomplete, unsafe or endpoint-bearing")


def exercise_takeout_faults(binary, workspace: Path, mock, invoke) -> None:
    """Prove exact convergence after a lost response at each import-only effect."""
    scenarios = [
        ("metadata", {"path_prefix": "/api/assets/", "method": "PUT"}, 1),
        ("album", {"path_prefix": "/api/albums", "method": "POST"}, 0),
        (
            "membership",
            {"path_prefix": "/api/albums/", "path_suffix": "/assets", "method": "PUT"},
            1,
        ),
    ]
    for name, selector, expected_retries in scenarios:
        source = workspace / f"takeout-{name}-source"
        year = source / "Takeout" / "Google Photos" / "Photos from 2024"
        album = source / "Takeout" / "Google Photos" / "Synthetic Album"
        year.mkdir(parents=True)
        album.mkdir(parents=True)
        media = f"synthetic Takeout {name} image\n".encode()
        file_name = f"synthetic-{name}.jpg"
        sidecar = json.dumps(
            {
                "title": file_name,
                "description": f"synthetic {name} description",
                "photoTakenTime": {"timestamp": "1704067200"},
            },
            separators=(",", ":"),
        ).encode()
        for directory in (year, album):
            (directory / file_name).write_bytes(media)
            (directory / f"{file_name}.json").write_bytes(sidecar)
        (album / "metadata.json").write_text(
            '{"title":"Synthetic Album"}', encoding="utf-8"
        )
        scenario = mock["default_scenario"]()
        scenario["fault"] = {
            "kind": "commit_lost_response",
            "times": 1,
            **selector,
        }
        with mock["running_mock"](scenario) as server:
            plan = invoke(
                binary,
                [
                    "plan",
                    "upload",
                    "google-takeout",
                    "--server",
                    server.url,
                    "--label",
                    f"synthetic-{name}",
                    "--buffer-bytes",
                    "4096",
                    str(source),
                ],
                with_key=True,
            )
            plan_path = workspace / f"takeout-{name}.json"
            plan_path.write_text(json.dumps(plan), encoding="utf-8")
            report = invoke(
                binary,
                [
                    "apply",
                    "upload",
                    "--server",
                    server.url,
                    "--plan",
                    str(plan_path),
                    "--buffer-bytes",
                    "4096",
                    "--source",
                    str(source),
                    "--checkpoint",
                    str(workspace / f"takeout-{name}.sqlite"),
                ],
                with_key=True,
            )
            snapshot = server.state.snapshot()
        if (
            report.get("failed") != 0
            or report.get("indeterminate") != 0
            or report.get("retried") != expected_retries
            or snapshot.get("asset_count") != 1
            or snapshot.get("metadata_count") != 1
            or snapshot.get("album_count") != 1
            or snapshot.get("album_memberships") != 1
        ):
            raise RuntimeError(f"Takeout {name} lost-response recovery did not converge")
