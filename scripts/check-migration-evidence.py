#!/usr/bin/env python3
"""Validate aggregate two-server disposable migration evidence."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_EVIDENCE = ROOT / "docs/evidence/phase11-disposable-migration-2026-08-24.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
FIXTURE = "ba1e5a34490aff9e6b29f673731503f4f86857b97c80ec3d8f32d098efbf672c"
IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}
MIGRATION_SUMMARY = {
    "assets": 3, "media_bytes": 214, "metadata_updates": 3,
    "album_creates": 1, "album_memberships": 1, "max_mutations": 8,
}
UPLOAD_SUMMARY = {
    "operations": 3, "media_bytes": 214, "xmp_sidecars": 0,
    "live_photo_pairs": 0, "metadata_updates": 3, "album_creates": 1,
    "album_memberships": 1, "max_mutations": 8,
}


class EvidenceError(ValueError):
    """Migration evidence is incomplete or inconsistent."""


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


def report(**changes: object) -> dict[str, object]:
    value: dict[str, object] = {
        "schema_version": 1, "dry_run": False, "planned": UPLOAD_SUMMARY,
        "would_upload": 0, "would_update_metadata": 0,
        "would_create_albums": 0, "would_add_album_memberships": 0,
        "created": 0, "duplicate": 0, "metadata_updated": 0,
        "albums_created": 0, "albums_reused": 0,
        "album_memberships_updated": 0, "resumed_effects": 0,
        "retried": 0, "failed": 0, "indeterminate": 0, "cancelled": False,
    }
    value.update(changes)
    return value


def validate_reports(value: object) -> None:
    expected = {
        "dry_run": report(
            dry_run=True, would_upload=3, would_update_metadata=3,
            would_create_albums=1, would_add_album_memberships=1,
        ),
        "first": report(
            created=3, metadata_updated=3, albums_created=1,
            album_memberships_updated=1,
        ),
        "resume": report(resumed_effects=8),
        "fresh_checkpoint_duplicate": report(
            duplicate=3, metadata_updated=3, albums_reused=1,
            album_memberships_updated=1,
        ),
    }
    if value != expected:
        raise EvidenceError("migration apply reports drifted")


def validate(path: Path) -> None:
    try:
        evidence = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load evidence: {error}") from error
    expected_keys = {
        "schema", "commit_sha", "binary_sha256", "fixture", "environment",
        "images", "server_version", "isolation", "plan", "reports",
        "postconditions", "cleanup",
    }
    if set(evidence) != expected_keys or evidence.get("schema") != "immich-migration-disposable-v1":
        raise EvidenceError("migration evidence schema drifted")
    commit = evidence.get("commit_sha")
    if not isinstance(commit, str) or COMMIT.fullmatch(commit) is None:
        raise EvidenceError("implementation commit is invalid")
    digest(evidence.get("binary_sha256"), "binary")
    if evidence.get("fixture") != {
        "kind": "synthetic", "license": "CC0-1.0", "manifest_sha256": FIXTURE,
    }:
        raise EvidenceError("synthetic fixture identity drifted")
    environment = object_value(evidence.get("environment"), "environment")
    if environment.get("os") != "Linux" or environment.get("architecture") != "x86_64":
        raise EvidenceError("unexpected disposable platform")
    if any(not isinstance(environment.get(key), str) or not environment[key]
           for key in ("docker", "compose")):
        raise EvidenceError("container environment is incomplete")
    if evidence.get("images") != IMAGES:
        raise EvidenceError("disposable image digest drifted")
    if evidence.get("server_version") != {
        "major": 3, "minor": 1, "patch": 0, "prerelease": None,
    }:
        raise EvidenceError("disposable server version drifted")
    if evidence.get("isolation") != {
        "instances": 2, "published_loopback": True,
        "production_endpoint": False, "production_credentials": False,
    }:
        raise EvidenceError("two-server isolation proof drifted")
    plan = object_value(evidence.get("plan"), "plan")
    digest(plan.get("sha256"), "migration plan")
    if plan != {
        "sha256": plan["sha256"], "summary": MIGRATION_SUMMARY,
        "source_byte_identical_after_apply": True,
    }:
        raise EvidenceError("migration plan proof drifted")
    validate_reports(evidence.get("reports"))
    if evidence.get("postconditions") != {
        "source_assets": 3, "source_owned_albums": 1,
        "destination_assets": 3, "destination_owned_albums": 1,
        "destination_album_members": 1, "descriptions": 3, "locations": 1,
    }:
        raise EvidenceError("migration postconditions drifted")
    if evidence.get("cleanup") != {
        "verified": True, "labelled_containers": 0, "labelled_volumes": 0,
        "labelled_networks": 0, "staging_files": 0,
        "credentials_removed": True, "seed_key_revoked": True,
    }:
        raise EvidenceError("migration cleanup proof drifted")


def main() -> int:
    try:
        validate(arguments().input)
    except EvidenceError as error:
        print(f"migration evidence check failed: {error}", file=sys.stderr)
        return 1
    print("two-server disposable migration evidence passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
