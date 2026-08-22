#!/usr/bin/env python3
"""Exercise the read-only archive CLI against synthetic mock originals."""

from __future__ import annotations

import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
MOCK = runpy.run_path(str(REPOSITORY_ROOT / "tests/oracle/mock_immich_server.py"))
SYNTHETIC_API_KEY = MOCK["SYNTHETIC_API_KEY"]

ASSETS = [
    {
        "id": "00000000-0000-4000-8000-000000000101",
        "filename": "synthetic-timeline.jpg",
        "body": b"synthetic archive timeline image\n",
        "type": "IMAGE",
        "visibility": "timeline",
        "live_photo_video_id": "00000000-0000-4000-8000-000000000105",
    },
    {
        "id": "00000000-0000-4000-8000-000000000102",
        "filename": "synthetic-archive.mp4",
        "body": b"synthetic archive video\n",
        "type": "VIDEO",
        "visibility": "archive",
    },
    {
        "id": "00000000-0000-4000-8000-000000000103",
        "filename": "synthetic-hidden.jpg",
        "body": b"synthetic hidden image\n",
        "type": "IMAGE",
        "visibility": "hidden",
    },
    {
        "id": "00000000-0000-4000-8000-000000000105",
        "filename": "synthetic-live-motion.mov",
        "body": b"synthetic linked live motion\n",
        "type": "VIDEO",
        "visibility": "linked",
    },
    {
        "id": "00000000-0000-4000-8000-000000000104",
        "filename": "synthetic-trashed.jpg",
        "body": b"synthetic excluded trash image\n",
        "type": "IMAGE",
        "visibility": "timeline",
        "trashed": True,
    },
]


def environment(with_key: bool = True, key_file: Path | None = None) -> dict[str, str]:
    """Return an allowlisted child environment without inherited credentials."""
    result = {"LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "/usr/bin:/bin")}
    if key_file is not None:
        result["IMMICH_RS_API_KEY_FILE"] = str(key_file)
    elif with_key:
        result["IMMICH_RS_API_KEY"] = SYNTHETIC_API_KEY
    return result


def invoke(
    binary: Path,
    arguments: list[str],
    *,
    success: bool = True,
    with_key: bool = True,
    key_file: Path | None = None,
) -> subprocess.CompletedProcess[bytes]:
    completed = subprocess.run(
        [str(binary), *arguments],
        check=False,
        capture_output=True,
        env=environment(with_key, key_file),
        timeout=20,
    )
    if success and (completed.returncode != 0 or completed.stderr):
        raise RuntimeError(
            f"archive CLI failed: exit={completed.returncode}, "
            f"stderr_bytes={len(completed.stderr)}"
        )
    return completed


def json_output(completed: subprocess.CompletedProcess[bytes]) -> dict[str, Any]:
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise RuntimeError("archive CLI did not return a JSON object")
    return value


def scenario() -> dict[str, Any]:
    selected = MOCK["default_scenario"]()
    selected["archive_assets"] = [dict(asset) for asset in ASSETS]
    return selected


def archive_arguments(server_url: str) -> list[str]:
    return [
        "plan",
        "archive",
        "immich",
        "--server",
        server_url,
        "--selection",
        "all",
        "--page-size",
        "1",
        "--max-assets",
        "4",
    ]


def verify_files(destination: Path) -> None:
    expected = {
        destination / "assets" / asset["id"] / asset["filename"]: asset["body"]
        for asset in ASSETS
        if not asset.get("trashed", False)
    }
    observed = {path: path.read_bytes() for path in destination.rglob("*") if path.is_file()}
    if observed != expected:
        raise RuntimeError("archived originals do not match the synthetic source bytes")


def exercise_convergence(binary: Path, workspace: Path) -> None:
    manifest_path = workspace / "archive-manifest.json"
    destination = workspace / "archive"
    with MOCK["running_mock"](scenario()) as server:
        manifest_result = invoke(binary, archive_arguments(server.url))
        manifest = json_output(manifest_result)
        if manifest.get("summary") != {"assets": 4, "media_bytes": 109}:
            raise RuntimeError("archive selection or pagination is incorrect")
        if server.url.encode() in manifest_result.stdout:
            raise RuntimeError("archive manifest exposed its endpoint")
        manifest_path.write_bytes(manifest_result.stdout)
        apply_arguments = [
            "apply",
            "archive",
            "--server",
            server.url,
            "--manifest",
            str(manifest_path),
            "--destination",
            str(destination),
        ]
        first = json_output(invoke(binary, apply_arguments))
        second = json_output(invoke(binary, apply_arguments))
        snapshot = server.state.snapshot()
    verify_files(destination)
    if first.get("downloaded") != 4 or first.get("bytes_written") != 109:
        raise RuntimeError("first archive apply did not download every original")
    if second.get("already_complete") != 4 or second.get("bytes_written") != 0:
        raise RuntimeError("archive rerun did not converge without downloads")
    if snapshot["committed_mutations"] or any(
        request["mutating"] for request in snapshot["requests"]
    ):
        raise RuntimeError("read-only archive reached a mutating mock path")


def exercise_secret_file(binary: Path, workspace: Path) -> None:
    key_file = workspace / "synthetic-api-key"
    key_file.write_text(f"{SYNTHETIC_API_KEY}\n", encoding="utf-8")
    key_file.chmod(0o400)
    with MOCK["running_mock"](scenario()) as server:
        completed = invoke(
            binary,
            archive_arguments(server.url),
            with_key=False,
            key_file=key_file,
        )
        conflicting_environment = environment(False, key_file)
        conflicting_environment["IMMICH_RS_API_KEY"] = SYNTHETIC_API_KEY
        conflict = subprocess.run(
            [str(binary), *archive_arguments(server.url)],
            check=False,
            capture_output=True,
            env=conflicting_environment,
            timeout=20,
        )
    if json_output(completed).get("summary", {}).get("assets") != 4:
        raise RuntimeError("API-key file did not authenticate the read-only client")
    if conflict.returncode != 5 or conflict.stdout or SYNTHETIC_API_KEY.encode() in conflict.stderr:
        raise RuntimeError("conflicting API-key sources did not fail closed")


def exercise_conflict(binary: Path, workspace: Path) -> None:
    manifest_path = workspace / "conflict-manifest.json"
    destination = workspace / "conflict-archive"
    with MOCK["running_mock"](scenario()) as server:
        result = invoke(binary, archive_arguments(server.url))
        manifest = json_output(result)
        manifest_path.write_bytes(result.stdout)
        first = manifest["assets"][0]
        conflict = destination / first["target_path"]
        conflict.parent.mkdir(parents=True)
        conflict.write_bytes(b"synthetic conflicting local bytes\n")
        completed = invoke(
            binary,
            [
                "apply",
                "archive",
                "--server",
                server.url,
                "--manifest",
                str(manifest_path),
                "--destination",
                str(destination),
            ],
            success=False,
        )
        snapshot = server.state.snapshot()
    if completed.returncode != 9 or completed.stdout:
        raise RuntimeError("local archive conflict did not fail with its stable exit class")
    if conflict.read_bytes() != b"synthetic conflicting local bytes\n":
        raise RuntimeError("archive conflict overwrote the existing local file")
    if snapshot["committed_mutations"]:
        raise RuntimeError("archive conflict caused a server mutation")


def exercise_faults(binary: Path) -> None:
    for kind in ("rate_limit", "server_error", "disconnect"):
        selected = scenario()
        selected["fault"] = {
            "kind": kind,
            "times": 1,
            "path_prefix": "/api/search/metadata",
        }
        with MOCK["running_mock"](selected) as server:
            completed = invoke(binary, archive_arguments(server.url), success=False)
            snapshot = server.state.snapshot()
        if completed.returncode != 7 or completed.stdout:
            raise RuntimeError(f"{kind} did not fail closed with the network exit class")
        if snapshot["committed_mutations"]:
            raise RuntimeError(f"{kind} caused a server mutation")


def exercise_stream_recovery(binary: Path, workspace: Path) -> None:
    manifest_path = workspace / "stream-manifest.json"
    destination = workspace / "stream-archive"
    selected = scenario()
    selected["fault"] = {
        "kind": "disconnect",
        "times": 1,
        "path_prefix": "/api/assets/00000000-0000-4000-8000-000000000101/original",
    }
    with MOCK["running_mock"](selected) as server:
        result = invoke(binary, archive_arguments(server.url))
        manifest_path.write_bytes(result.stdout)
        arguments = [
            "apply",
            "archive",
            "--server",
            server.url,
            "--manifest",
            str(manifest_path),
            "--destination",
            str(destination),
        ]
        failed = invoke(binary, arguments, success=False)
        recovered = json_output(invoke(binary, arguments))
    if failed.returncode != 7 or recovered.get("downloaded") != 4:
        raise RuntimeError("stream disconnect did not recover on an idempotent rerun")
    if list(destination.rglob("*.immich-rs.part")):
        raise RuntimeError("stream failure left a partial archive file")


def exercise_checksum_refusal(binary: Path, workspace: Path) -> None:
    manifest_path = workspace / "checksum-manifest.json"
    destination = workspace / "checksum-archive"
    selected = scenario()
    selected["archive_assets"][0]["download_body"] = b"synthetic mismatching stream bytes!\n"
    with MOCK["running_mock"](selected) as server:
        result = invoke(binary, archive_arguments(server.url))
        manifest_path.write_bytes(result.stdout)
        failed = invoke(
            binary,
            [
                "apply",
                "archive",
                "--server",
                server.url,
                "--manifest",
                str(manifest_path),
                "--destination",
                str(destination),
            ],
            success=False,
        )
    if failed.returncode != 7 or any(path.is_file() for path in destination.rglob("*")):
        raise RuntimeError("checksum mismatch committed or retained unverified bytes")


def exercise_cancellation(binary: Path, workspace: Path) -> None:
    manifest_path = workspace / "cancel-manifest.json"
    destination = workspace / "cancel-archive"
    selected = scenario()
    selected["fault"] = {
        "kind": "timeout",
        "times": 1,
        "delay_ms": 500,
        "path_prefix": "/api/assets/00000000-0000-4000-8000-000000000101/original",
    }
    with MOCK["running_mock"](selected) as server:
        result = invoke(binary, archive_arguments(server.url))
        manifest_path.write_bytes(result.stdout)
        arguments = [
            "apply",
            "archive",
            "--server",
            server.url,
            "--manifest",
            str(manifest_path),
            "--destination",
            str(destination),
        ]
        process = subprocess.Popen(
            [str(binary), *arguments],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment(),
        )
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            paths = [request["path"] for request in server.state.snapshot()["requests"]]
            if any(path.endswith("/original") for path in paths):
                break
            if process.poll() is not None:
                stdout, stderr = process.communicate()
                raise RuntimeError(
                    "archive cancellation target exited before original stream: "
                    f"exit={process.returncode}, stdout_bytes={len(stdout)}, "
                    f"stderr_bytes={len(stderr)}"
                )
            time.sleep(0.01)
        else:
            process.kill()
            process.wait(timeout=5)
            raise RuntimeError("archive cancellation did not reach the original stream")
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=10)
        time.sleep(0.6)
        recovered = json_output(invoke(binary, arguments))
    if process.returncode != 130 or stdout or SYNTHETIC_API_KEY.encode() in stderr:
        raise RuntimeError(
            f"archive cancellation did not exit cleanly: exit={process.returncode}, "
            f"stdout_bytes={len(stdout)}, stderr_bytes={len(stderr)}"
        )
    if recovered.get("downloaded") != 4 or list(destination.rglob("*.immich-rs.part")):
        raise RuntimeError("archive did not converge after cancellation")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: run_phase5_mock.py <immich-rs-binary>", file=sys.stderr)
        return 2
    binary = Path(sys.argv[1]).resolve()
    if not binary.is_file():
        print("immich-rs test binary is missing", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="immich-rs-phase5-mock-") as temporary:
        workspace = Path(temporary).resolve(strict=True)
        exercise_convergence(binary, workspace)
        exercise_secret_file(binary, workspace)
        exercise_conflict(binary, workspace)
        exercise_faults(binary)
        exercise_stream_recovery(binary, workspace)
        exercise_checksum_refusal(binary, workspace)
        exercise_cancellation(binary, workspace)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
