#!/usr/bin/env python3
"""Exercise Web Console auth, metrics and cold state recovery on synthetic data."""

from __future__ import annotations

import argparse
from http.client import HTTPConnection
from http.cookies import SimpleCookie
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import signal
import socket
import subprocess
import tempfile
import time
from typing import BinaryIO, NoReturn
from urllib.parse import urlencode


ROOT = Path(__file__).resolve().parent.parent
ARTIFACT_ROOT = ROOT / ".artifacts"
SHA = re.compile(r"^[0-9a-f]{40}$")
CSRF = re.compile(r'name="csrf" value="([^"]+)"')
MAX_RESPONSE = 1024 * 1024


class RehearsalError(RuntimeError):
    """The disposable Web Console rehearsal failed closed."""


class Browser:
    def __init__(self, port: int) -> None:
        self.port = port
        self.cookies: dict[str, str] = {}

    def request(
        self,
        method: str,
        path: str,
        body: bytes = b"",
        headers: dict[str, str] | None = None,
        *,
        cookies: bool = True,
    ) -> tuple[int, dict[str, str], str]:
        request_headers = dict(headers or {})
        if cookies and self.cookies:
            request_headers["Cookie"] = "; ".join(
                f"{key}={value}" for key, value in sorted(self.cookies.items())
            )
        connection = HTTPConnection("127.0.0.1", self.port, timeout=3)
        try:
            connection.request(method, path, body=body, headers=request_headers)
            response = connection.getresponse()
            payload = response.read(MAX_RESPONSE + 1)
            if len(payload) > MAX_RESPONSE:
                raise RehearsalError("web response exceeded the rehearsal bound")
            for name, value in response.getheaders():
                if name.lower() == "set-cookie":
                    parsed = SimpleCookie()
                    parsed.load(value)
                    for key, morsel in parsed.items():
                        if morsel.value:
                            self.cookies[key] = morsel.value
                        else:
                            self.cookies.pop(key, None)
            response_headers = {name.lower(): value for name, value in response.getheaders()}
            return response.status, response_headers, payload.decode("utf-8")
        finally:
            connection.close()


def fail(message: str) -> NoReturn:
    raise RehearsalError(message)


def reserve_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def write_private(path: Path, value: str) -> None:
    path.write_text(f"{value}\n", encoding="utf-8")
    path.chmod(0o600)


def write_config(root: Path, state: Path, port: int) -> Path:
    config = root / f"web-{state.name}.toml"
    config.write_text(
        f'''schema_version = 1

[web]
listen_address = "127.0.0.1:{port}"
public_origin = "http://127.0.0.1:{port}"
bootstrap_secret_file = "{root / 'bootstrap.secret'}"
history_state_id = "console"

[web.metrics]
bearer_token_file = "{root / 'metrics.secret'}"
allowed_cidrs = ["127.0.0.1/32"]

[[sources]]
id = "synthetic"
label = "Synthetic source"
allowed_root = "{root / 'source'}"
relative_root = "."
generation = 1

[[states]]
id = "console"
label = "Synthetic state"
allowed_root = "{state}"
relative_root = "."
generation = 1
''',
        encoding="utf-8",
    )
    return config


def start(binary: Path, config: Path, log: Path) -> tuple[subprocess.Popen[bytes], BinaryIO]:
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("IMMICH_RS_WEB_")
    }
    handle = log.open("wb")
    try:
        process = subprocess.Popen(
            [str(binary), "--config", str(config)],
            stdin=subprocess.DEVNULL,
            stdout=handle,
            stderr=subprocess.STDOUT,
            env=environment,
        )
    except OSError:
        handle.close()
        raise
    return process, handle


def wait_ready(process: subprocess.Popen[bytes], browser: Browser) -> None:
    for _attempt in range(100):
        if process.poll() is not None:
            fail("web process exited before becoming ready")
        try:
            status, _, _ = browser.request("GET", "/")
            if status == 200:
                return
        except OSError:
            pass
        time.sleep(0.05)
    fail("web process did not become ready within five seconds")


def stop(process: subprocess.Popen[bytes], handle: BinaryIO) -> None:
    try:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
                fail("web process did not complete bounded graceful shutdown")
        if process.returncode != 0:
            fail("web process returned a non-zero status")
    finally:
        handle.close()


def csrf(body: str) -> str:
    match = CSRF.search(body)
    if match is None:
        fail("CSRF field is missing")
    return match.group(1)


def pair(browser: Browser, origin: str, bootstrap: str) -> str:
    status, _, page = browser.request("GET", "/pair")
    if status != 200:
        fail("pairing page was unavailable")
    body = urlencode({"csrf": csrf(page), "secret": bootstrap}).encode()
    status, _, _ = browser.request(
        "POST",
        "/pair",
        body,
        {"Content-Type": "application/x-www-form-urlencoded", "Origin": origin},
    )
    if status != 303:
        fail("pairing did not redirect")
    status, _, dashboard = browser.request("GET", "/")
    if status != 200 or "Synthetic source" not in dashboard:
        fail("paired dashboard is unavailable")
    return csrf(dashboard)


def scan(browser: Browser, origin: str, token: str) -> None:
    body = urlencode({"csrf": token}).encode()
    status, headers, _ = browser.request(
        "POST",
        "/sources/synthetic/scan",
        body,
        {"Content-Type": "application/x-www-form-urlencoded", "Origin": origin},
    )
    location = headers.get("location")
    if status != 303 or not location:
        fail("scan admission did not return a job location")
    for _attempt in range(200):
        status, _, page = browser.request("GET", location)
        if status == 200 and "Completed ·" in page:
            return
        time.sleep(0.025)
    fail("scan did not complete within the bounded wait")


def metrics(browser: Browser, token: str) -> dict[str, int]:
    bearer = {"Authorization": f"Bearer {token}"}
    status, _, body = browser.request("GET", "/metrics", headers=bearer, cookies=False)
    if status != 200 or token in body or "{" in body:
        fail("machine metrics response is invalid")
    values = {}
    for name in ("sessions_active", "jobs_completed", "history_rows"):
        match = re.search(rf"^immich_rs_web_{name} ([0-9]+)$", body, re.MULTILINE)
        if match is None:
            fail(f"required metric is absent: {name}")
        values[name] = int(match.group(1))
    return values


def tree_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        relative = path.relative_to(root).as_posix().encode()
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        digest.update(path.stat().st_size.to_bytes(8, "big"))
        with path.open("rb") as source:
            while chunk := source.read(64 * 1024):
                digest.update(chunk)
    return digest.hexdigest()


def file_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(64 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def exercise(binary: Path, workspace: Path) -> dict[str, object]:
    source = workspace / "source"
    original = workspace / "state-original"
    backup = workspace / "state-backup"
    restored = workspace / "state-restored"
    for directory in (source, original):
        directory.mkdir(mode=0o700)
    (source / "synthetic.jpg").write_bytes(b"synthetic immich-rs web rehearsal\n")
    bootstrap = secrets.token_hex(16)
    token = secrets.token_hex(32)
    write_private(workspace / "bootstrap.secret", bootstrap)
    write_private(workspace / "metrics.secret", token)
    port = reserve_port()
    origin = f"http://127.0.0.1:{port}"

    first, first_log = start(binary, write_config(workspace, original, port), workspace / "first.log")
    browser = Browser(port)
    try:
        wait_ready(first, browser)
        denied, _, _ = browser.request("GET", f"/metrics?token={token}", cookies=False)
        if denied != 401:
            fail("query-string metrics credential was accepted")
        before = metrics(browser, token)
        scan(browser, origin, pair(browser, origin, bootstrap))
        after = metrics(browser, token)
    finally:
        stop(first, first_log)
    if before != {"sessions_active": 0, "jobs_completed": 0, "history_rows": 0}:
        fail("initial metrics were not empty")
    if after["sessions_active"] != 1 or after["history_rows"] != 1:
        fail("completed scan metrics were not observable")

    shutil.copytree(original, backup, copy_function=shutil.copy2)
    shutil.copytree(backup, restored, copy_function=shutil.copy2)
    state_digest = tree_digest(original)
    if state_digest != tree_digest(backup) or state_digest != tree_digest(restored):
        fail("cold backup and restore changed the state tree")

    second, second_log = start(binary, write_config(workspace, restored, port), workspace / "second.log")
    recovered_browser = Browser(port)
    try:
        wait_ready(second, recovered_browser)
        recovered = metrics(recovered_browser, token)
        bearer = {"Authorization": f"Bearer {token}"}
        denied, _, _ = recovered_browser.request("GET", "/history", headers=bearer, cookies=False)
        if denied != 401:
            fail("metrics bearer escaped its route scope")
        pair(recovered_browser, origin, bootstrap)
        status, _, history = recovered_browser.request("GET", "/history")
        if status != 200 or "No terminal work has been recorded" in history:
            fail("restored terminal history is unavailable")
    finally:
        stop(second, second_log)
    if recovered != {"sessions_active": 0, "jobs_completed": 0, "history_rows": 1}:
        fail("restart did not preserve only durable metrics")
    return {
        "schema": "web-disposable-recovery-v1",
        "authentication": {
            "browser_pairing": True,
            "machine_metrics": True,
            "query_token_rejected": True,
            "bearer_route_scoped": True,
        },
        "workflow": {"folder_scan_completed": True, "history_rows": 1},
        "recovery": {
            "copied_while_stopped": True,
            "state_tree_sha256": state_digest,
            "sessions_after_restart": recovered["sessions_active"],
            "history_after_restart": recovered["history_rows"],
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    binary = arguments.binary.resolve()
    output = arguments.output.resolve()
    if os.name != "posix" or not binary.is_file() or not os.access(binary, os.X_OK):
        fail("the rehearsal requires an executable Web Console binary on POSIX")
    if not SHA.fullmatch(arguments.commit_sha) or output.exists():
        fail("invalid commit or pre-existing output")
    ARTIFACT_ROOT.mkdir(exist_ok=True)
    if ARTIFACT_ROOT.resolve() not in output.parents:
        fail("output must be below .artifacts")
    with tempfile.TemporaryDirectory(prefix="immich-rs-web-rehearsal.") as temporary:
        workspace = Path(temporary)
        report = exercise(binary, workspace)
        report.update(
            {
                "commit_sha": arguments.commit_sha,
                "binary_sha256": file_digest(binary),
                "environment": {"os": platform.system(), "architecture": platform.machine()},
                "fixture": {"kind": "synthetic", "license": "CC0-1.0", "assets": 1},
                "isolation": {"loopback_only": True, "production_access": False},
            }
        )
    report["cleanup"] = {"workspace_removed": not workspace.exists(), "processes_remaining": 0}
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_output = output.with_suffix(output.suffix + ".tmp")
    temporary_output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary_output, output)
    print("disposable Web Console recovery rehearsal passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RehearsalError as error:
        print(f"disposable Web Console rehearsal failed: {error}", file=os.sys.stderr)
        raise SystemExit(1) from error
