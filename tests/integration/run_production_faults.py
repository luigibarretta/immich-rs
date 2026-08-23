#!/usr/bin/env python3
"""Exercise production authorization through verified TLS and the synthetic mock."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import ipaddress
import json
import os
from pathlib import Path
import runpy
import signal
import socket
import subprocess
import sys
import time
from typing import Any, Iterator
from urllib.parse import urlparse

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
MOCK = runpy.run_path(str(REPOSITORY_ROOT / "tests/oracle/mock_immich_server.py"))
SYNTHETIC_API_KEY = MOCK["SYNTHETIC_API_KEY"]


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--listen-host", required=True)
    parser.add_argument("--ca-certificate", type=Path, required=True)
    parser.add_argument("--server-certificate", type=Path, required=True)
    parser.add_argument("--server-key", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    values = parser.parse_args()
    try:
        address = ipaddress.ip_address(values.listen_host)
    except ValueError as error:
        parser.error(f"listen host must be an IP address: {error}")
    if address.is_loopback or not address.is_private:
        parser.error("listen host must be a private non-loopback address")
    if not values.binary.is_file() or not os.access(values.binary, os.X_OK):
        parser.error("binary must be an executable file")
    for path in (values.ca_certificate, values.server_certificate, values.server_key):
        if not path.is_file() or path.is_symlink():
            parser.error("TLS inputs must be regular files")
    workspace = values.workspace.resolve()
    if not str(workspace).startswith("/tmp/immich-rs-production."):
        parser.error("workspace must remain inside the disposable production directory")
    workspace.mkdir(parents=True, exist_ok=False)
    values.workspace = workspace
    return values


def child_environment(with_key: bool) -> dict[str, str]:
    environment = {"LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "/usr/bin:/bin")}
    if with_key:
        environment["IMMICH_RS_API_KEY"] = SYNTHETIC_API_KEY
    return environment


def invoke(
    values: argparse.Namespace,
    command: list[str],
    *,
    with_key: bool,
) -> tuple[int, dict[str, Any] | None, bytes]:
    completed = subprocess.run(
        [str(values.binary), *command],
        check=False,
        capture_output=True,
        env=child_environment(with_key),
        timeout=30,
    )
    parsed = None
    if completed.stdout:
        candidate = json.loads(completed.stdout)
        if not isinstance(candidate, dict):
            raise RuntimeError("CLI output was not a JSON object")
        parsed = candidate
    return completed.returncode, parsed, completed.stderr


def available_port(host: str) -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind((host, 0))
        return int(listener.getsockname()[1])


@contextmanager
def secure_mock(
    values: argparse.Namespace,
    scenario: dict[str, Any] | None = None,
) -> Iterator[tuple[Any, str]]:
    with MOCK["running_mock"](scenario) as server:
        target_port = urlparse(server.url).port
        if target_port is None:
            raise RuntimeError("mock target port is absent")
        listen_port = available_port(values.listen_host)
        process = subprocess.Popen(
            [
                sys.executable,
                str(REPOSITORY_ROOT / "scripts/tls-forward.py"),
                "--listen-host",
                values.listen_host,
                "--listen-port",
                str(listen_port),
                "--target-port",
                str(target_port),
                "--certificate",
                str(values.server_certificate),
                "--private-key",
                str(values.server_key),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    with socket.create_connection((values.listen_host, listen_port), timeout=0.2):
                        break
                except OSError:
                    if process.poll() is not None:
                        raise RuntimeError("TLS forwarder exited during startup")
                    time.sleep(0.02)
            else:
                raise RuntimeError("TLS forwarder did not become ready")
            yield server, f"https://{values.listen_host}:{listen_port}"
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def write_source(path: Path, label: str) -> None:
    path.mkdir()
    (path / "synthetic.jpg").write_bytes(f"synthetic {label} payload\n".encode())


def production_plan(
    values: argparse.Namespace,
    source: Path,
    endpoint: str,
    stem: str,
) -> tuple[Path, list[str]]:
    plan_path = values.workspace / f"{stem}-plan.json"
    code, plan, stderr = invoke(
        values,
        [
            "plan",
            "upload",
            "folder",
            "--server",
            endpoint,
            "--authorize-production-read",
            "--ca-certificate",
            str(values.ca_certificate),
            str(source),
        ],
        with_key=True,
    )
    if code != 0 or plan is None or stderr:
        raise RuntimeError("production mock planning failed")
    plan_path.write_text(json.dumps(plan, sort_keys=True), encoding="utf-8")
    code, inspection, stderr = invoke(
        values,
        ["inspect", "upload-plan", "--plan", str(plan_path)],
        with_key=False,
    )
    if code != 0 or inspection is None or stderr:
        raise RuntimeError("production plan inspection failed")
    checkpoint = values.workspace / f"{stem}-checkpoint.sqlite"
    apply = [
        "apply",
        "upload",
        "--server",
        endpoint,
        "--ca-certificate",
        str(values.ca_certificate),
        "--plan",
        str(plan_path),
        "--source",
        str(source),
        "--checkpoint",
        str(checkpoint),
        "--authorize-production-read",
        "--authorize-production-write",
        "--confirm-plan-sha256",
        str(inspection["plan_sha256"]),
        "--expected-operations",
        str(inspection["operations"]),
        "--backup-reference",
        f"synthetic-{stem}-backup",
    ]
    return checkpoint, apply


def exercise_fault(values: argparse.Namespace, kind: str) -> dict[str, int]:
    source = values.workspace / f"{kind}-source"
    write_source(source, kind)
    scenario = MOCK["default_scenario"]()
    scenario["fault"] = {"kind": kind, "times": 1, "path_prefix": "/api/assets"}
    with secure_mock(values, scenario) as (server, endpoint):
        _checkpoint, apply = production_plan(values, source, endpoint, kind)
        code, report, stderr = invoke(values, apply, with_key=True)
        snapshot = server.state.snapshot()
    if code != 0 or report is None or stderr:
        raise RuntimeError(f"{kind} production apply failed")
    converged = int(report.get("created", 0)) + int(report.get("duplicate", 0))
    if converged != 1 or report.get("retried") != 1 or snapshot["asset_count"] != 1:
        raise RuntimeError(f"{kind} did not converge exactly once")
    return {"converged": converged, "retried": int(report["retried"])}


def exercise_refusals(values: argparse.Namespace) -> dict[str, int]:
    source = values.workspace / "refusal-source"
    write_source(source, "refusal")
    incompatible = MOCK["default_scenario"]()
    incompatible["version"] = {"major": 3, "minor": 2, "patch": 0}
    with secure_mock(values, incompatible) as (server, endpoint):
        code, output, _stderr = invoke(
            values,
            [
                "plan",
                "upload",
                "folder",
                "--server",
                endpoint,
                "--authorize-production-read",
                "--ca-certificate",
                str(values.ca_certificate),
                str(source),
            ],
            with_key=True,
        )
        mutations = server.state.snapshot()["committed_mutations"]
    if code != 6 or output is not None or mutations:
        raise RuntimeError("incompatible production server did not fail closed")
    with secure_mock(values) as (server, endpoint):
        code, output, _stderr = invoke(
            values,
            [
                "plan",
                "archive",
                "immich",
                "--server",
                endpoint,
                "--authorize-production-read",
                "--ca-certificate",
                str(values.ca_certificate),
            ],
            with_key=False,
        )
        requests = len(server.state.snapshot()["requests"])
    if code != 5 or output is not None or requests != 0:
        raise RuntimeError("missing production key did not fail before network")
    return {"incompatible_exit": 6, "missing_key_exit": 5}


def exercise_cancellation(values: argparse.Namespace) -> dict[str, int | bool]:
    source = values.workspace / "cancellation-source"
    write_source(source, "cancellation")
    scenario = MOCK["default_scenario"]()
    scenario["fault"] = {
        "kind": "timeout",
        "times": 1,
        "path_prefix": "/api/assets/bulk-upload-check",
        "delay_ms": 500,
    }
    with secure_mock(values, scenario) as (server, endpoint):
        _checkpoint, apply = production_plan(values, source, endpoint, "cancellation")
        process = subprocess.Popen(
            [str(values.binary), *apply],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=child_environment(True),
        )
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            paths = [request["path"] for request in server.state.snapshot()["requests"]]
            if "/api/assets/bulk-upload-check" in paths:
                break
            if process.poll() is not None:
                raise RuntimeError("production cancellation target exited early")
            time.sleep(0.01)
        else:
            process.kill()
            process.wait(timeout=5)
            raise RuntimeError("production cancellation target was not reached")
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=10)
        code, resumed, resumed_stderr = invoke(values, apply, with_key=True)
        assets = server.state.snapshot()["asset_count"]
    if (
        process.returncode != 130
        or stdout
        or SYNTHETIC_API_KEY.encode() in stderr
        or code != 0
        or resumed is None
        or resumed_stderr
        or resumed.get("created") != 1
        or assets != 1
    ):
        raise RuntimeError("production cancellation did not resume exactly once")
    return {"cancelled_exit": 130, "resumed": True}


def main() -> int:
    values = arguments()
    faults = {
        kind: exercise_fault(values, kind)
        for kind in ("rate_limit", "server_error", "disconnect", "commit_lost_response")
    }
    evidence = {
        "schema": "phase7-production-faults-v1",
        "faults": faults,
        "refusals": exercise_refusals(values),
        "cancellation": exercise_cancellation(values),
    }
    json.dump(evidence, sys.stdout, sort_keys=True, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
