"""Contract tests for the bounded synthetic Immich server."""

from __future__ import annotations

from copy import deepcopy
import http.client
import importlib.util
import json
from pathlib import Path
import sys
import time
import unittest
import urllib.error
import urllib.request

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_mock():
    path = REPOSITORY_ROOT / "tests" / "oracle" / "mock_immich_server.py"
    spec = importlib.util.spec_from_file_location("tested_mock_immich", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_mock_immich"] = module
    spec.loader.exec_module(module)
    return module


mock = load_mock()


def request(server, path: str, method: str = "GET", api_key: str | None = None, body: bytes = b""):
    headers = {"Content-Type": "application/json"}
    if api_key is not None:
        headers["x-api-key"] = api_key
    request_value = urllib.request.Request(server.url + path, data=body if method != "GET" else None, method=method, headers=headers)
    return urllib.request.urlopen(request_value, timeout=2)


class MockImmichServerTests(unittest.TestCase):
    def test_authentication_and_version_contract(self) -> None:
        with mock.running_mock() as server:
            with self.assertRaises(urllib.error.HTTPError) as error:
                request(server, "/api/server/version")
            self.assertEqual(error.exception.code, 401)
            with request(server, "/api/server/version", api_key=mock.SYNTHETIC_API_KEY) as response:
                self.assertEqual(
                    json.load(response),
                    {"major": 3, "minor": 1, "patch": 0, "prerelease": None},
                )

    def test_incompatible_version_is_explicit(self) -> None:
        scenario = mock.default_scenario()
        scenario["version"] = {"major": 99, "minor": 0, "patch": 0}
        with mock.running_mock(scenario) as server:
            with request(server, "/api/server/version", api_key=mock.SYNTHETIC_API_KEY) as response:
                self.assertEqual(json.load(response)["major"], 99)

    def test_rate_limit_and_server_error_are_injected(self) -> None:
        for kind, status in (("rate_limit", 429), ("server_error", 503)):
            with self.subTest(kind=kind):
                scenario = mock.default_scenario()
                scenario["fault"] = {"kind": kind, "times": 1, "path_prefix": "/api/"}
                with mock.running_mock(scenario) as server:
                    with self.assertRaises(urllib.error.HTTPError) as error:
                        request(server, "/api/server/version", api_key=mock.SYNTHETIC_API_KEY)
                    self.assertEqual(error.exception.code, status)

    def test_timeout_is_injected_with_a_bounded_delay(self) -> None:
        scenario = mock.default_scenario()
        scenario["fault"] = {
            "kind": "timeout",
            "times": 1,
            "path_prefix": "/api/",
            "delay_ms": 120,
        }
        started = time.monotonic()
        with mock.running_mock(scenario) as server:
            with request(server, "/api/server/version", api_key=mock.SYNTHETIC_API_KEY) as response:
                self.assertEqual(response.status, 200)
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 0.08)
        self.assertLess(elapsed, 1.0)

    def test_disconnect_is_observable(self) -> None:
        scenario = mock.default_scenario()
        scenario["fault"] = {"kind": "disconnect", "times": 1, "path_prefix": "/api/"}
        with mock.running_mock(scenario) as server:
            with self.assertRaises((http.client.RemoteDisconnected, urllib.error.URLError)):
                request(server, "/api/server/version", api_key=mock.SYNTHETIC_API_KEY)

    def test_lost_response_records_a_committed_mutation(self) -> None:
        scenario = mock.default_scenario()
        scenario["fault"] = {
            "kind": "commit_lost_response",
            "times": 1,
            "path_prefix": "/api/assets",
        }
        with mock.running_mock(scenario) as server:
            with self.assertRaises((http.client.RemoteDisconnected, urllib.error.URLError)):
                request(server, "/api/assets", method="POST", api_key=mock.SYNTHETIC_API_KEY, body=b"{}")
            snapshot = server.state.snapshot()
        self.assertEqual(len(snapshot["committed_mutations"]), 1)
        self.assertTrue(snapshot["committed_mutations"][0]["mutating"])

    def test_read_only_post_does_not_count_as_mutation(self) -> None:
        with mock.running_mock() as server:
            with request(
                server,
                "/api/search/metadata",
                method="POST",
                api_key=mock.SYNTHETIC_API_KEY,
                body=b"{}",
            ) as response:
                self.assertEqual(response.status, 200)
            snapshot = server.state.snapshot()
        self.assertFalse(snapshot["requests"][0]["mutating"])
        self.assertFalse(snapshot["committed_mutations"])

    def test_bulk_check_converges_after_one_upload(self) -> None:
        checksum = "synthetic-checksum"
        bulk = json.dumps({"assets": [{"id": "synthetic-operation", "checksum": checksum}]}).encode()
        with mock.running_mock() as server:
            with request(
                server,
                "/api/assets/bulk-upload-check",
                method="POST",
                api_key=mock.SYNTHETIC_API_KEY,
                body=bulk,
            ) as response:
                self.assertEqual(json.load(response)["results"][0]["action"], "accept")
            upload_request = urllib.request.Request(
                server.url + "/api/assets",
                data=b"synthetic multipart body",
                method="POST",
                headers={"x-api-key": mock.SYNTHETIC_API_KEY, "x-immich-checksum": checksum},
            )
            with urllib.request.urlopen(upload_request, timeout=2) as response:
                self.assertEqual(json.load(response)["status"], "created")
            with request(
                server,
                "/api/assets/bulk-upload-check",
                method="POST",
                api_key=mock.SYNTHETIC_API_KEY,
                body=bulk,
            ) as response:
                self.assertEqual(json.load(response)["results"][0]["action"], "reject")
            snapshot = server.state.snapshot()
        self.assertEqual(snapshot["asset_count"], 1)

    def test_scenario_rejects_non_synthetic_credentials(self) -> None:
        scenario = deepcopy(mock.default_scenario())
        scenario["api_key"] = "not-the-declared-test-constant"
        with self.assertRaises(mock.MockConfigurationError):
            mock.MockImmichServer(scenario)


if __name__ == "__main__":
    unittest.main()
