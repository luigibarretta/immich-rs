#!/usr/bin/env python3
"""Validate committed disposable Web Console recovery evidence."""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_INPUT = ROOT / "docs/evidence/web-disposable-recovery-2026-09-10.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")


class EvidenceError(ValueError):
    """The Web Console recovery report is incomplete or inconsistent."""


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def validate(path: Path) -> None:
    try:
        report = object_value(json.loads(path.read_text(encoding="utf-8")), "report")
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read evidence: {error}") from error
    encoded = json.dumps(report, sort_keys=True)
    forbidden = ("/tmp/", "api_key", "password", "accessToken", "Authorization")
    if any(value in encoded for value in forbidden):
        raise EvidenceError("evidence contains a runtime path or credential name")
    if set(report) != {
        "schema", "authentication", "binary_sha256", "cleanup", "commit_sha",
        "environment", "fixture", "isolation", "recovery", "workflow",
    } or report.get("schema") != "web-disposable-recovery-v1":
        raise EvidenceError("schema or top-level fields drifted")
    if COMMIT.fullmatch(str(report.get("commit_sha"))) is None:
        raise EvidenceError("implementation commit is invalid")
    if SHA256.fullmatch(str(report.get("binary_sha256"))) is None:
        raise EvidenceError("binary digest is invalid")
    if report.get("environment") != {"architecture": "x86_64", "os": "Linux"}:
        raise EvidenceError("environment drifted")
    if report.get("fixture") != {"assets": 1, "kind": "synthetic", "license": "CC0-1.0"}:
        raise EvidenceError("fixture provenance drifted")
    if report.get("isolation") != {"loopback_only": True, "production_access": False}:
        raise EvidenceError("isolation boundary drifted")
    if report.get("authentication") != {
        "bearer_route_scoped": True, "browser_pairing": True,
        "machine_metrics": True, "query_token_rejected": True,
    }:
        raise EvidenceError("authentication postconditions drifted")
    if report.get("workflow") != {"folder_scan_completed": True, "history_rows": 1}:
        raise EvidenceError("workflow postconditions drifted")
    recovery = object_value(report.get("recovery"), "recovery")
    if (
        set(recovery) != {
            "copied_while_stopped", "history_after_restart",
            "sessions_after_restart", "state_tree_sha256",
        }
        or recovery.get("copied_while_stopped") is not True
        or recovery.get("history_after_restart") != 1
        or recovery.get("sessions_after_restart") != 0
        or SHA256.fullmatch(str(recovery.get("state_tree_sha256"))) is None
    ):
        raise EvidenceError("cold recovery postconditions drifted")
    if report.get("cleanup") != {"processes_remaining": 0, "workspace_removed": True}:
        raise EvidenceError("cleanup postconditions drifted")


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) == 2 else DEFAULT_INPUT
    if len(sys.argv) > 2:
        print("usage: check-web-evidence.py [report.json]", file=sys.stderr)
        return 2
    try:
        validate(path)
    except EvidenceError as error:
        print(f"Web Console evidence check failed: {error}", file=sys.stderr)
        return 1
    print("Web Console recovery evidence passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
