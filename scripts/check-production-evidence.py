#!/usr/bin/env python3
"""Validate aggregate evidence from the isolated production-HTTPS gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_EVIDENCE = ROOT / "docs/evidence/phase7-disposable-production-2026-08-23.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
EXPECTED_IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}


class EvidenceError(ValueError):
    """Production gate evidence is incomplete or inconsistent."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=DEFAULT_EVIDENCE)
    return parser.parse_args()


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def digest(value: object, label: str) -> None:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise EvidenceError(f"{label} is not a SHA-256 digest")


def validate_report(report: object, expected_counter: str) -> None:
    value = object_value(report, expected_counter)
    expected = {
        "schema_version": 1,
        "dry_run": expected_counter == "would_upload",
        "planned": 4,
        "would_upload": 0,
        "created": 0,
        "duplicate": 0,
        "resumed": 0,
        "retried": 0,
        "failed": 0,
        "indeterminate": 0,
        "cancelled": False,
    }
    expected[expected_counter] = 4
    if value != expected:
        raise EvidenceError(f"{expected_counter} report drifted")


def validate_faults(value: object) -> None:
    matrix = object_value(value, "TLS fault matrix")
    if matrix.get("schema") != "phase7-production-faults-v1":
        raise EvidenceError("unsupported TLS fault schema")
    faults = object_value(matrix.get("faults"), "TLS faults")
    expected_faults = {"rate_limit", "server_error", "disconnect", "commit_lost_response"}
    if set(faults) != expected_faults:
        raise EvidenceError("TLS fault cases drifted")
    if any(report != {"converged": 1, "retried": 1} for report in faults.values()):
        raise EvidenceError("TLS fault recovery did not converge once")
    if matrix.get("refusals") != {"incompatible_exit": 6, "missing_key_exit": 5}:
        raise EvidenceError("TLS refusal classes drifted")
    if matrix.get("cancellation") != {"cancelled_exit": 130, "resumed": True}:
        raise EvidenceError("TLS cancellation contract drifted")


def validate(path: Path) -> None:
    try:
        evidence = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load evidence: {error}") from error
    expected_keys = {
        "schema",
        "commit_sha",
        "binary_sha256",
        "images",
        "tls",
        "authorization",
        "server_version",
        "reports",
        "archive_assets",
        "asset_count",
        "cleanup",
    }
    if set(evidence) != expected_keys or evidence.get("schema") != "phase7-disposable-production-v1":
        raise EvidenceError("production evidence schema drifted")
    commit = evidence.get("commit_sha")
    if not isinstance(commit, str) or COMMIT.fullmatch(commit) is None:
        raise EvidenceError("implementation commit is invalid")
    digest(evidence.get("binary_sha256"), "binary")
    if evidence.get("images") != EXPECTED_IMAGES:
        raise EvidenceError("production gate image digests drifted")
    tls = object_value(evidence.get("tls"), "TLS")
    digest(tls.get("custom_ca_sha256"), "custom CA")
    if tls.get("hostname_verification") is not True or tls.get("untrusted_certificate_exit") != 7:
        raise EvidenceError("TLS verification contract drifted")
    validate_faults(tls.get("fault_matrix"))
    if evidence.get("authorization") != {
        "plan_digest_matched": True,
        "operation_budget": 4,
        "backup_reference_hashed": True,
        "mismatch_exit": 2,
    }:
        raise EvidenceError("production authorization contract drifted")
    if evidence.get("server_version") != {
        "major": 3,
        "minor": 1,
        "patch": 0,
        "prerelease": None,
    }:
        raise EvidenceError("production server version drifted")
    reports = object_value(evidence.get("reports"), "reports")
    if set(reports) != {"dry_run", "first", "resume", "duplicate"}:
        raise EvidenceError("production reports are incomplete")
    for name, counter in (
        ("dry_run", "would_upload"),
        ("first", "created"),
        ("resume", "resumed"),
        ("duplicate", "duplicate"),
    ):
        validate_report(reports.get(name), counter)
    if evidence.get("archive_assets") != 4 or evidence.get("asset_count") != 3:
        raise EvidenceError("production server observables drifted")
    if evidence.get("cleanup") != {
        "verified": True,
        "labelled_containers": 0,
        "labelled_volumes": 0,
        "labelled_networks": 0,
        "private_keys_removed": True,
        "credentials_removed": True,
    }:
        raise EvidenceError("production cleanup proof drifted")


def main() -> int:
    try:
        validate(arguments().input)
    except EvidenceError as error:
        print(f"production evidence check failed: {error}", file=sys.stderr)
        return 1
    print("production HTTPS evidence passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
