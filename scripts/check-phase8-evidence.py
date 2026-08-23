#!/usr/bin/env python3
"""Validate committed aggregate evidence for the Google Takeout import gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_EVIDENCE = ROOT / "docs/evidence/phase8-disposable-takeout-2026-08-23.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
FIXTURE_DIGEST = "ba1e5a34490aff9e6b29f673731503f4f86857b97c80ec3d8f32d098efbf672c"
EXPECTED_IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}
PLANNED = {
    "operations": 3,
    "media_bytes": 214,
    "xmp_sidecars": 0,
    "live_photo_pairs": 0,
    "metadata_updates": 3,
    "album_creates": 1,
    "album_memberships": 1,
    "max_mutations": 8,
}


class EvidenceError(ValueError):
    """Phase 8 evidence is incomplete or inconsistent."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=DEFAULT_EVIDENCE)
    return parser.parse_args()


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def digest(value: object, label: str) -> None:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise EvidenceError(f"{label} is not a SHA-256 digest")


def expected_report(**changes: object) -> dict[str, object]:
    report: dict[str, object] = {
        "schema_version": 1,
        "dry_run": False,
        "planned": PLANNED,
        "would_upload": 0,
        "would_update_metadata": 0,
        "would_create_albums": 0,
        "would_add_album_memberships": 0,
        "created": 0,
        "duplicate": 0,
        "metadata_updated": 0,
        "albums_created": 0,
        "albums_reused": 0,
        "album_memberships_updated": 0,
        "resumed_effects": 0,
        "retried": 0,
        "failed": 0,
        "indeterminate": 0,
        "cancelled": False,
    }
    report.update(changes)
    return report


def validate_reports(value: object) -> None:
    reports = object_value(value, "reports")
    expected = {
        "dry_run": expected_report(
            dry_run=True,
            would_upload=3,
            would_update_metadata=3,
            would_create_albums=1,
            would_add_album_memberships=1,
        ),
        "first": expected_report(
            created=3,
            metadata_updated=3,
            albums_created=1,
            album_memberships_updated=1,
        ),
        "resume": expected_report(resumed_effects=8),
        "archive_duplicate": expected_report(
            duplicate=3,
            metadata_updated=3,
            albums_reused=1,
            album_memberships_updated=1,
        ),
    }
    if reports != expected:
        raise EvidenceError("Takeout apply reports drifted")


def validate(path: Path) -> None:
    try:
        evidence = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load evidence: {error}") from error
    expected_keys = {
        "schema", "commit_sha", "binary_sha256", "fixture", "images", "tls",
        "server_version", "plan", "authorization", "reports", "postconditions", "cleanup",
    }
    if set(evidence) != expected_keys or evidence.get("schema") != "phase8-disposable-takeout-v1":
        raise EvidenceError("Phase 8 evidence schema drifted")
    commit = evidence.get("commit_sha")
    if not isinstance(commit, str) or COMMIT.fullmatch(commit) is None:
        raise EvidenceError("implementation commit is invalid")
    digest(evidence.get("binary_sha256"), "binary")
    fixture = object_value(evidence.get("fixture"), "fixture")
    if fixture != {
        "schema": "fixture-manifest-v2",
        "manifest_sha256": FIXTURE_DIGEST,
        "provenance": "synthetic-CC0-1.0",
    }:
        raise EvidenceError("synthetic fixture identity drifted")
    if evidence.get("images") != EXPECTED_IMAGES:
        raise EvidenceError("disposable image digests drifted")
    tls = object_value(evidence.get("tls"), "TLS")
    digest(tls.get("custom_ca_sha256"), "custom CA")
    if set(tls) != {"custom_ca_sha256", "hostname_verification"} or tls.get("hostname_verification") is not True:
        raise EvidenceError("TLS contract drifted")
    if evidence.get("server_version") != {"major": 3, "minor": 1, "patch": 0, "prerelease": None}:
        raise EvidenceError("disposable server version drifted")
    plan = object_value(evidence.get("plan"), "plan")
    digest(plan.get("sha256"), "plan")
    if plan != {"directory_zip_byte_identical": True, "sha256": plan["sha256"], **PLANNED}:
        raise EvidenceError("Takeout plan contract drifted")
    if evidence.get("authorization") != {
        "maximum_mutation_budget": 8,
        "backup_reference_hashed": True,
        "mismatch_exit": 2,
    }:
        raise EvidenceError("production authorization proof drifted")
    validate_reports(evidence.get("reports"))
    if evidence.get("postconditions") != {
        "schema": "phase8-takeout-postconditions-v1",
        "assets": 3,
        "metadata_assignments": 3,
        "albums": 1,
        "album_memberships": 1,
        "exact": True,
    }:
        raise EvidenceError("server postconditions drifted")
    if evidence.get("cleanup") != {
        "verified": True,
        "labelled_containers": 0,
        "labelled_volumes": 0,
        "labelled_networks": 0,
        "staging_files": 0,
        "private_keys_removed": True,
        "credentials_removed": True,
    }:
        raise EvidenceError("disposable cleanup proof drifted")


def main() -> int:
    try:
        validate(arguments().input)
    except EvidenceError as error:
        print(f"Phase 8 evidence check failed: {error}", file=sys.stderr)
        return 1
    print("Phase 8 disposable Takeout evidence passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
