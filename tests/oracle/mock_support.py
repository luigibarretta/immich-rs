"""Thread-safe state and mutation classification for the synthetic Immich mock."""

from __future__ import annotations

from threading import Lock
from typing import Any
from pathlib import Path
import runpy

MOCK_SCHEMA = "mock-immich-v1"
IMPORT = runpy.run_path(str(Path(__file__).resolve().with_name("mock_import.py")))
MockImportState = IMPORT["MockImportState"]


class MockState:
    """Thread-safe observable server state with synthetic checksum convergence."""

    def __init__(self, scenario: dict[str, Any]):
        self.scenario = scenario
        self.requests: list[dict[str, Any]] = []
        self.committed_mutations: list[dict[str, Any]] = []
        self._assets: dict[str, str] = {}
        self.imports = MockImportState(scenario)
        self._lock = Lock()
        fault = scenario.get("fault", {})
        self._fault_remaining = int(fault.get("times", 0)) if isinstance(fault, dict) else 0

    def record(self, request: dict[str, Any]) -> None:
        with self._lock:
            self.requests.append(request)

    def record_commit(self, request: dict[str, Any]) -> None:
        with self._lock:
            self.committed_mutations.append(request)

    def check_asset(self, operation_id: str, checksum: str) -> dict[str, str]:
        with self._lock:
            asset_id = self._assets.get(checksum)
        if asset_id is None:
            return {"id": operation_id, "action": "accept"}
        return {"id": operation_id, "action": "reject", "assetId": asset_id}

    def commit_asset(self, checksum: str) -> tuple[str, str]:
        with self._lock:
            existing = self._assets.get(checksum)
            if existing is not None:
                return existing, "duplicate"
            sequence = len(self._assets) + 2
            asset_id = f"00000000-0000-4000-8000-{sequence:012d}"
            self._assets[checksum] = asset_id
            return asset_id, "created"

    def consume_fault(self, path: str, method: str) -> str | None:
        fault = self.scenario.get("fault", {})
        if not isinstance(fault, dict):
            return None
        prefix = fault.get("path_prefix", "/api/")
        suffix = fault.get("path_suffix", "")
        selected_method = fault.get("method")
        kind = fault.get("kind")
        if (
            not isinstance(prefix, str)
            or not isinstance(suffix, str)
            or not isinstance(kind, str)
            or not path.startswith(prefix)
            or not path.endswith(suffix)
            or (selected_method is not None and selected_method != method)
        ):
            return None
        with self._lock:
            if self._fault_remaining <= 0:
                return None
            self._fault_remaining -= 1
        return kind

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return {
                "schema": MOCK_SCHEMA,
                "requests": [dict(request) for request in self.requests],
                "committed_mutations": [dict(request) for request in self.committed_mutations],
                "asset_count": len(self._assets),
                **self.imports.snapshot(),
            }


def is_mutating_request(method: str, path: str) -> bool:
    """Classify semantic mutations rather than treating every POST as a write."""
    if method in {"PUT", "PATCH", "DELETE"}:
        return True
    if method != "POST":
        return False
    read_only_posts = (
        "/api/search/",
        "/api/assets/bulk-upload-check",
        "/api/assets/exist",
        "/api/duplicates",
    )
    return not path.startswith(read_only_posts)
