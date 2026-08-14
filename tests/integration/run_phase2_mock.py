#!/usr/bin/env python3
"""Exercise Phase-2 CLI behavior against only the bounded synthetic mock."""

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


def child_environment(with_key: bool) -> dict[str, str]:
    """Return an allowlisted environment without inherited Immich credentials."""
    environment = {
        "LANG": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    }
    if with_key:
        environment["IMMICH_RS_API_KEY"] = SYNTHETIC_API_KEY
    return environment


def invoke(binary: Path, arguments: list[str], *, with_key: bool) -> dict[str, Any]:
    completed = invoke_process(binary, arguments, with_key=with_key)
    if completed.returncode != 0 or completed.stderr:
        raise RuntimeError(
            f"Phase-2 CLI failed closed unexpectedly: exit={completed.returncode}, "
            f"stderr_bytes={len(completed.stderr)}"
        )
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise RuntimeError("Phase-2 CLI did not return a JSON object")
    return value


def invoke_process(
    binary: Path, arguments: list[str], *, with_key: bool
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [str(binary), *arguments],
        check=False,
        capture_output=True,
        env=child_environment(with_key),
        timeout=20,
    )


def write_matrix(source: Path) -> None:
    source.mkdir()
    (source / "image.jpg").write_bytes(b"synthetic standalone image\n")
    (source / "image.xmp").write_bytes(b"<x:xmpmeta synthetic='true'/>\n")
    (source / "clip.mp4").write_bytes(b"synthetic standalone video\n")
    (source / "live.jpg").write_bytes(b"synthetic live still\n")
    (source / "live.mov").write_bytes(b"synthetic live motion\n")


def create_plan(binary: Path, source: Path, server_url: str, plan_path: Path) -> dict[str, Any]:
    plan = invoke(
        binary,
        [
            "plan",
            "upload",
            "folder",
            "--server",
            server_url,
            "--label",
            "synthetic-phase2",
            "--buffer-bytes",
            "4096",
            str(source),
        ],
        with_key=True,
    )
    plan_path.write_text(json.dumps(plan, sort_keys=True), encoding="utf-8")
    return plan


def exercise_matrix(binary: Path, workspace: Path) -> None:
    source = workspace / "matrix-source"
    write_matrix(source)
    plan_path = workspace / "matrix-plan.json"
    checkpoint = workspace / "matrix-checkpoint.sqlite"
    duplicate_checkpoint = workspace / "matrix-duplicate.sqlite"
    with MOCK["running_mock"]() as server:
        plan = create_plan(binary, source, server.url, plan_path)
        if plan.get("summary", {}).get("operations") != 4 or server.url in json.dumps(plan):
            raise RuntimeError("matrix upload plan is incomplete or exposes its endpoint")
        requests_before_dry_run = len(server.state.snapshot()["requests"])
        dry_run = invoke(
            binary,
            [
                "apply",
                "upload",
                "--dry-run",
                "--plan",
                str(plan_path),
                "--source",
                str(source),
                "--checkpoint",
                str(checkpoint),
                "--buffer-bytes",
                "4096",
            ],
            with_key=False,
        )
        if dry_run.get("would_upload") != 4 or checkpoint.exists():
            raise RuntimeError("dry-run mutated its checkpoint or returned incorrect counts")
        if len(server.state.snapshot()["requests"]) != requests_before_dry_run:
            raise RuntimeError("dry-run reached the synthetic server")
        apply_arguments = [
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
            "--buffer-bytes",
            "4096",
        ]
        first = invoke(binary, apply_arguments, with_key=True)
        second = invoke(binary, apply_arguments, with_key=True)
        duplicate_arguments = list(apply_arguments)
        checkpoint_index = duplicate_arguments.index("--checkpoint") + 1
        duplicate_arguments[checkpoint_index] = str(duplicate_checkpoint)
        duplicate = invoke(binary, duplicate_arguments, with_key=True)
        snapshot = server.state.snapshot()
    if first.get("created") != 4 or second.get("resumed") != 4:
        raise RuntimeError("first apply or checkpoint resume did not converge")
    if duplicate.get("duplicate") != 4 or snapshot.get("asset_count") != 4:
        raise RuntimeError("remote duplicate convergence created extra assets")


def exercise_fault(binary: Path, workspace: Path, kind: str) -> None:
    source = workspace / f"{kind}-source"
    source.mkdir()
    (source / "fault.jpg").write_bytes(f"synthetic {kind} payload\n".encode())
    plan_path = workspace / f"{kind}-plan.json"
    checkpoint = workspace / f"{kind}-checkpoint.sqlite"
    scenario = MOCK["default_scenario"]()
    scenario["fault"] = {
        "kind": kind,
        "times": 1,
        "path_prefix": "/api/assets",
    }
    with MOCK["running_mock"](scenario) as server:
        create_plan(binary, source, server.url, plan_path)
        report = invoke(
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
                "--buffer-bytes",
                "4096",
            ],
            with_key=True,
        )
        snapshot = server.state.snapshot()
    converged = report.get("created", 0) + report.get("duplicate", 0)
    if converged != 1 or report.get("retried") != 1 or snapshot.get("asset_count") != 1:
        raise RuntimeError(f"{kind} recovery did not converge exactly once")


def exercise_refusals(binary: Path, workspace: Path) -> None:
    source = workspace / "refusal-source"
    source.mkdir()
    (source / "refusal.jpg").write_bytes(b"synthetic refusal payload\n")
    scenarios = []
    incompatible = MOCK["default_scenario"]()
    incompatible["version"] = {"major": 3, "minor": 2, "patch": 0}
    scenarios.append((incompatible, 6))
    oversized = MOCK["default_scenario"]()
    oversized["responses"]["user"]["syntheticPadding"] = "x" * 70_000
    scenarios.append((oversized, 70))
    for scenario, expected_exit in scenarios:
        with MOCK["running_mock"](scenario) as server:
            completed = invoke_process(
                binary,
                [
                    "plan",
                    "upload",
                    "folder",
                    "--server",
                    server.url,
                    str(source),
                ],
                with_key=True,
            )
            snapshot = server.state.snapshot()
        if completed.returncode != expected_exit or completed.stdout:
            raise RuntimeError("compatibility or response limit did not fail closed")
        if SYNTHETIC_API_KEY.encode() in completed.stderr or snapshot["committed_mutations"]:
            raise RuntimeError("failed request exposed a secret or mutated the server")
    with MOCK["running_mock"]() as server:
        missing_key = invoke_process(
            binary,
            ["plan", "upload", "folder", "--server", server.url, str(source)],
            with_key=False,
        )
        request_count = len(server.state.snapshot()["requests"])
    if missing_key.returncode != 5 or missing_key.stdout or request_count != 0:
        raise RuntimeError("missing authentication did not fail before network access")


def exercise_cancellation(binary: Path, workspace: Path) -> None:
    source = workspace / "cancellation-source"
    source.mkdir()
    (source / "cancel.jpg").write_bytes(b"synthetic cancellation payload\n")
    plan_path = workspace / "cancellation-plan.json"
    checkpoint = workspace / "cancellation-checkpoint.sqlite"
    scenario = MOCK["default_scenario"]()
    scenario["fault"] = {
        "kind": "timeout",
        "times": 1,
        "path_prefix": "/api/assets/bulk-upload-check",
        "delay_ms": 500,
    }
    with MOCK["running_mock"](scenario) as server:
        create_plan(binary, source, server.url, plan_path)
        arguments = [
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
            "--buffer-bytes",
            "4096",
        ]
        process = subprocess.Popen(
            [str(binary), *arguments],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=child_environment(True),
        )
        for _attempt in range(100):
            paths = [request["path"] for request in server.state.snapshot()["requests"]]
            if "/api/assets/bulk-upload-check" in paths:
                break
            time.sleep(0.01)
        else:
            process.kill()
            process.wait(timeout=2)
            raise RuntimeError("cancellation request did not reach the mock")
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=2)
        time.sleep(0.6)
        resumed = invoke(binary, arguments, with_key=True)
        snapshot = server.state.snapshot()
    if process.returncode != 130 or stdout or SYNTHETIC_API_KEY.encode() in stderr:
        raise RuntimeError("cancellation did not exit cleanly without secret output")
    if resumed.get("created") != 1 or snapshot.get("asset_count") != 1:
        raise RuntimeError("post-cancellation apply did not converge exactly once")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: run_phase2_mock.py <immich-rs-binary>", file=sys.stderr)
        return 2
    binary = Path(sys.argv[1]).resolve()
    if not binary.is_file():
        print("immich-rs test binary is missing", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="immich-rs-phase2-mock-") as temporary:
        workspace = Path(temporary)
        exercise_matrix(binary, workspace)
        exercise_fault(binary, workspace, "rate_limit")
        exercise_fault(binary, workspace, "server_error")
        exercise_fault(binary, workspace, "disconnect")
        exercise_fault(binary, workspace, "commit_lost_response")
        exercise_refusals(binary, workspace)
        exercise_cancellation(binary, workspace)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
