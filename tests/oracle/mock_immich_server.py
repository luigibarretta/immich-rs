#!/usr/bin/env python3
"""Bounded synthetic Immich HTTP server with deterministic fault injection."""

from __future__ import annotations

from collections.abc import Iterator
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import runpy
import socket
from threading import Thread
import time
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlsplit

MOCK_SCHEMA = "mock-immich-v1"
SYNTHETIC_API_KEY = "synthetic-oracle-key"
MAX_REQUEST_BODY_BYTES = 1_048_576
RESPONSE_FIXTURE_PATH = Path(__file__).resolve().parent / "server-fixtures" / "immich-v3.1.json"
SUPPORT = runpy.run_path(str(Path(__file__).resolve().with_name("mock_support.py")))
MockState = SUPPORT["MockState"]
is_mutating_request = SUPPORT["is_mutating_request"]


class MockConfigurationError(ValueError):
    """The requested mock scenario is unsafe or malformed."""


def _validate_scenario(scenario: dict[str, Any]) -> None:
    if scenario.get("schema") != MOCK_SCHEMA:
        raise MockConfigurationError(f"mock schema must be {MOCK_SCHEMA}")
    if scenario.get("api_key") != SYNTHETIC_API_KEY:
        raise MockConfigurationError("mock API key must use the synthetic constant")
    version = scenario.get("version")
    if not isinstance(version, dict) or any(not isinstance(version.get(key), int) for key in ("major", "minor", "patch")):
        raise MockConfigurationError("mock version must contain integer major, minor and patch values")
    fault = scenario.get("fault", {})
    if fault:
        if not isinstance(fault, dict) or fault.get("kind") not in {
            "timeout",
            "rate_limit",
            "server_error",
            "disconnect",
            "commit_lost_response",
        }:
            raise MockConfigurationError("unsupported mock fault")
        times = fault.get("times")
        if not isinstance(times, int) or not 1 <= times <= 100:
            raise MockConfigurationError("fault times must be in 1..100")
        delay_ms = fault.get("delay_ms", 0)
        if not isinstance(delay_ms, int) or not 0 <= delay_ms <= 60_000:
            raise MockConfigurationError("fault delay must be in 0..60000 milliseconds")


def default_scenario() -> dict[str, Any]:
    """Return a supported, authenticated Immich v3.1 synthetic scenario."""
    try:
        responses = json.loads(RESPONSE_FIXTURE_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise MockConfigurationError(f"cannot load mock response fixture: {error}") from error
    if responses.get("schema") != "mock-immich-responses-v1":
        raise MockConfigurationError("unsupported mock response fixture schema")
    provenance = responses.get("provenance")
    if (
        not isinstance(responses.get("fixture_id"), str)
        or not isinstance(provenance, dict)
        or provenance.get("kind") != "synthetic"
        or provenance.get("license") != "CC0-1.0"
    ):
        raise MockConfigurationError("mock response fixture lacks synthetic provenance")
    version = responses.get("version")
    if not isinstance(version, dict):
        raise MockConfigurationError("mock response fixture lacks a version")
    return {
        "schema": MOCK_SCHEMA,
        "api_key": SYNTHETIC_API_KEY,
        "version": dict(version),
        "fault": {},
        "responses": responses,
    }


def _normalized_target(raw_target: str) -> tuple[str, str]:
    parsed = urlsplit(raw_target)
    query = urlencode(sorted(parse_qsl(parsed.query, keep_blank_values=True)))
    return parsed.path, query


class _Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "immich-rs-mock/1"
    sys_version = ""

    @property
    def state(self) -> MockState:
        server = self.server
        state = getattr(server, "mock_state", None)
        if not isinstance(state, MockState):
            raise MockConfigurationError("mock server state is unavailable")
        return state

    def log_message(self, _format: str, *_arguments: object) -> None:
        return

    def _read_body(self) -> bytes | None:
        if self.headers.get("Transfer-Encoding", "").casefold() == "chunked":
            return self._read_chunked_body()
        raw_length = self.headers.get("Content-Length", "0")
        try:
            length = int(raw_length)
        except ValueError:
            self._json_response(400, {"message": "invalid synthetic content length"})
            return None
        if length < 0 or length > MAX_REQUEST_BODY_BYTES:
            self._json_response(413, {"message": "synthetic request body limit exceeded"})
            return None
        return self.rfile.read(length)

    def _read_chunked_body(self) -> bytes | None:
        body = bytearray()
        while True:
            raw_size = self.rfile.readline(128)
            if not raw_size.endswith(b"\r\n"):
                self._json_response(400, {"message": "invalid synthetic chunk framing"})
                return None
            try:
                size = int(raw_size.split(b";", 1)[0].strip(), 16)
            except ValueError:
                self._json_response(400, {"message": "invalid synthetic chunk size"})
                return None
            if size == 0:
                self.rfile.readline(128)
                return bytes(body)
            if size < 0 or len(body) + size > MAX_REQUEST_BODY_BYTES:
                self._json_response(413, {"message": "synthetic request body limit exceeded"})
                return None
            chunk = self.rfile.read(size)
            terminator = self.rfile.read(2)
            if len(chunk) != size or terminator != b"\r\n":
                self._json_response(400, {"message": "invalid synthetic chunk body"})
                return None
            body.extend(chunk)

    def _json_response(self, status: int, payload: object, headers: dict[str, str] | None = None) -> None:
        body = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        if headers:
            for key, value in sorted(headers.items()):
                self.send_header(key, value)
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _disconnect(self) -> None:
        try:
            self.connection.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self.connection.close()

    def _dispatch(self) -> None:
        path, query = _normalized_target(self.path)
        body = self._read_body()
        if body is None:
            return
        mutating = is_mutating_request(self.command, path)
        json_body = None
        if body and self.headers.get("Content-Type", "").split(";", 1)[0] == "application/json":
            try:
                json_body = json.loads(body)
            except (UnicodeError, json.JSONDecodeError):
                json_body = "<invalid-json>"
        request = {
            "method": self.command,
            "path": path,
            "query": query,
            "body_sha256": hashlib.sha256(body).hexdigest(),
            "body_bytes": len(body),
            "authenticated": self.headers.get("x-api-key") == SYNTHETIC_API_KEY,
            "mutating": mutating,
            "json_body": json_body,
        }
        self.state.record(request)

        scenario = self.state.scenario
        if not request["authenticated"]:
            self._json_response(401, {"message": "synthetic authentication rejected"})
            return

        configured_fault = scenario.get("fault", {})
        skip_lost_response_precheck = (
            isinstance(configured_fault, dict)
            and configured_fault.get("kind") == "commit_lost_response"
            and not mutating
        )
        fault = None if skip_lost_response_precheck else self.state.consume_fault(path)
        if fault == "timeout":
            delay_ms = configured_fault.get("delay_ms", 100) if isinstance(configured_fault, dict) else 100
            time.sleep(delay_ms / 1000)
        elif fault == "rate_limit":
            self._json_response(429, {"message": "synthetic rate limit"}, {"Retry-After": "0"})
            return
        elif fault == "server_error":
            self._json_response(503, {"message": "synthetic server error"})
            return
        elif fault == "disconnect":
            self._disconnect()
            return
        elif fault == "commit_lost_response" and mutating:
            checksum = self.headers.get("x-immich-checksum", "synthetic-unidentified")
            self.state.commit_asset(checksum)
            self.state.record_commit(request)
            self._disconnect()
            return

        version = scenario["version"]
        responses = scenario["responses"]
        if path == "/api/server/ping" and self.command == "GET":
            self._json_response(200, {"res": "pong"})
        elif path == "/api/server/version" and self.command == "GET":
            self._json_response(200, version)
        elif path == "/api/server/about" and self.command == "GET":
            self._json_response(
                200,
                {
                    "version": f"v{version['major']}.{version['minor']}.{version['patch']}",
                },
            )
        elif path == "/api/users/me" and self.command == "GET":
            self._json_response(200, responses["user"])
        elif path == "/api/server/config" and self.command == "GET":
            self._json_response(200, {"trashDays": 30, "userDeleteDelay": 7})
        elif path == "/api/server/media-types" and self.command == "GET":
            self._json_response(200, responses["media_types"])
        elif path == "/api/jobs" and self.command == "GET":
            job_status = {
                "jobCounts": {
                    "active": 0,
                    "completed": 0,
                    "delayed": 0,
                    "failed": 0,
                    "paused": 0,
                    "waiting": 0,
                },
                "queueStatus": {"isActive": True, "isPaused": False},
            }
            self._json_response(
                200,
                {
                    name: job_status
                    for name in (
                        "faceDetection",
                        "metadataExtraction",
                        "smartSearch",
                        "thumbnailGeneration",
                        "videoConversion",
                    )
                },
            )
        elif path == "/api/assets/statistics" and self.command == "GET":
            self._json_response(200, responses["asset_statistics"])
        elif path.startswith("/api/search/") and self.command == "POST":
            self._json_response(200, responses["search_result"])
        elif path == "/api/assets/bulk-upload-check" and self.command == "POST":
            assets = json_body.get("assets") if isinstance(json_body, dict) else None
            if not isinstance(assets, list) or any(
                not isinstance(asset, dict)
                or not isinstance(asset.get("id"), str)
                or not isinstance(asset.get("checksum"), str)
                for asset in assets
            ):
                self._json_response(400, {"message": "invalid synthetic bulk check"})
                return
            results = [self.state.check_asset(asset["id"], asset["checksum"]) for asset in assets]
            self._json_response(200, {"results": results})
        elif path == "/api/assets" and self.command == "POST":
            checksum = self.headers.get("x-immich-checksum", "synthetic-unidentified")
            asset_id, status = self.state.commit_asset(checksum)
            if status == "created":
                self.state.record_commit(request)
            self._json_response(201, {"id": asset_id, "status": status})
        elif mutating:
            self.state.record_commit(request)
            self._json_response(200, {"status": "synthetic-commit"})
        elif self.command == "GET":
            self._json_response(200, [])
        else:
            self._json_response(200, {})

    do_GET = _dispatch
    do_POST = _dispatch
    do_PUT = _dispatch
    do_PATCH = _dispatch
    do_DELETE = _dispatch


class MockImmichServer:
    """One bounded HTTP server thread with explicit shutdown and observations."""

    def __init__(self, scenario: dict[str, Any] | None = None):
        selected = default_scenario() if scenario is None else scenario
        _validate_scenario(selected)
        self.state = MockState(selected)
        self._server = HTTPServer(("127.0.0.1", 0), _Handler)
        self._server.mock_state = self.state
        self._thread = Thread(target=self._server.serve_forever, name="mock-immich-v1")

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def start(self) -> None:
        self._thread.start()

    def close(self) -> None:
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=5)
        if self._thread.is_alive():
            raise RuntimeError("mock Immich server did not stop cleanly")

    def __enter__(self) -> MockImmichServer:
        self.start()
        return self

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()


@contextmanager
def running_mock(scenario: dict[str, Any] | None = None) -> Iterator[MockImmichServer]:
    """Run one mock server and always join its bounded thread."""
    server = MockImmichServer(scenario)
    server.start()
    try:
        yield server
    finally:
        server.close()
