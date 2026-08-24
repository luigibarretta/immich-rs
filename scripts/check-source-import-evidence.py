#!/usr/bin/env python3
"""Validate aggregate Apple Photos and Picasa disposable import evidence."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = ROOT / "docs" / "evidence"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}
ADAPTERS = {
    "apple-photos": {
        "fixture": "3469ca97f565c1649c2f40ba67cbca9a2c2a6a6a8ba25287293d1662cc106d0b",
        "plan": {
            "operations": 5, "media_bytes": 342, "xmp_sidecars": 1,
            "live_photo_pairs": 1, "album_creates": 1,
            "album_memberships": 1, "max_mutations": 7,
        },
        "postconditions": {
            "assets": 5, "owned_albums": 1, "album_members": 5,
            "live_photo_links": 1, "descriptions": 0,
        },
    },
    "picasa": {
        "fixture": "1571164014b18d6adc281fb61f61a0488ed6e9f127f7bbd773835de17a2dcae1",
        "plan": {
            "operations": 4, "media_bytes": 262, "xmp_sidecars": 1,
            "live_photo_pairs": 1, "metadata_updates": 2,
            "album_creates": 1, "album_memberships": 1, "max_mutations": 8,
        },
        "postconditions": {
            "assets": 4, "owned_albums": 1, "album_members": 4,
            "live_photo_links": 1, "descriptions": 1,
        },
    },
}


class EvidenceError(ValueError):
    """Evidence is incomplete or inconsistent."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, action="append")
    return parser.parse_args()


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def digest(value: object, label: str) -> None:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise EvidenceError(f"{label} is not a SHA-256 digest")


def expected_report(plan: dict[str, object], **changes: object) -> dict[str, object]:
    report: dict[str, object] = {
        "schema_version": 1, "dry_run": False, "planned": plan,
        "would_upload": 0, "would_update_metadata": 0,
        "would_create_albums": 0, "would_add_album_memberships": 0,
        "created": 0, "duplicate": 0, "metadata_updated": 0,
        "albums_created": 0, "albums_reused": 0,
        "album_memberships_updated": 0, "resumed_effects": 0,
        "retried": 0, "failed": 0, "indeterminate": 0, "cancelled": False,
    }
    report.update(changes)
    return report


def validate_reports(value: object, adapter: str, plan: dict[str, object]) -> None:
    reports = object_value(value, "reports")
    assets = int(plan["operations"])
    metadata = int(plan.get("metadata_updates", 0))
    mutations = int(plan["max_mutations"])
    expected = {
        "dry_run": expected_report(
            plan, dry_run=True, would_upload=assets,
            would_update_metadata=metadata, would_create_albums=1,
            would_add_album_memberships=1,
        ),
        "first": expected_report(
            plan, created=assets, metadata_updated=metadata,
            albums_created=1, album_memberships_updated=1,
        ),
        "resume": expected_report(plan, resumed_effects=mutations),
        "fresh_checkpoint_duplicate": expected_report(
            plan, duplicate=assets, metadata_updated=metadata,
            albums_reused=1, album_memberships_updated=1,
        ),
    }
    if reports != expected:
        raise EvidenceError(f"{adapter} apply reports drifted")


def validate(path: Path) -> str:
    try:
        evidence = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load evidence: {error}") from error
    keys = {
        "schema", "adapter", "commit_sha", "binary_sha256", "fixture",
        "environment", "images", "server_version", "isolation", "plan",
        "reports", "postconditions", "cleanup",
    }
    if set(evidence) != keys or evidence.get("schema") != "source-import-disposable-v1":
        raise EvidenceError("source import evidence schema drifted")
    adapter = evidence.get("adapter")
    if not isinstance(adapter, str) or adapter not in ADAPTERS:
        raise EvidenceError("unsupported source adapter")
    expected = ADAPTERS[adapter]
    commit = evidence.get("commit_sha")
    if not isinstance(commit, str) or COMMIT.fullmatch(commit) is None:
        raise EvidenceError("implementation commit is invalid")
    digest(evidence.get("binary_sha256"), "binary")
    fixture = object_value(evidence.get("fixture"), "fixture")
    if fixture != {
        "kind": "synthetic", "license": "CC0-1.0",
        "manifest_sha256": expected["fixture"],
    }:
        raise EvidenceError(f"{adapter} fixture identity drifted")
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
        "published_loopback": True, "production_endpoint": False,
        "production_credentials": False,
    }:
        raise EvidenceError("disposable isolation proof drifted")
    plan = object_value(evidence.get("plan"), "plan")
    if plan != {
        "directory_zip_semantically_equivalent": True,
        "transport_timestamps_excluded": True, "summary": expected["plan"],
    }:
        raise EvidenceError(f"{adapter} plan contract drifted")
    validate_reports(evidence.get("reports"), adapter, expected["plan"])
    if evidence.get("postconditions") != expected["postconditions"]:
        raise EvidenceError(f"{adapter} server postconditions drifted")
    if evidence.get("cleanup") != {
        "verified": True, "labelled_containers": 0, "labelled_volumes": 0,
        "labelled_networks": 0, "staging_files": 0,
        "credentials_removed": True,
    }:
        raise EvidenceError("disposable cleanup proof drifted")
    return adapter


def main() -> int:
    requested = arguments().input
    paths = requested or sorted(EVIDENCE_ROOT.glob("phase*-source-import-*.json"))
    try:
        if not paths:
            raise EvidenceError("source import disposable evidence is missing")
        adapters = {validate(path) for path in paths}
        if requested is None and adapters != set(ADAPTERS):
            raise EvidenceError("Apple Photos and Picasa evidence are both required")
    except EvidenceError as error:
        print(f"source import evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"source import disposable evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
