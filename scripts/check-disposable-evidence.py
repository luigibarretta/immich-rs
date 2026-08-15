#!/usr/bin/env python3
"""Validate committed aggregate evidence from the isolated Phase-2 gate."""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys
from typing import Any

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = REPOSITORY_ROOT / "docs" / "evidence"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
EXPECTED_IMAGES = {
    "server": "ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa",
    "valkey": "docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411",
    "database": "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23",
}
EXPECTED_COMMANDS = [
    "plan upload folder",
    "apply upload --dry-run",
    "apply upload",
    "apply upload (checkpoint resume)",
    "apply upload (fresh checkpoint duplicate check)",
]


class EvidenceError(ValueError):
    """Committed disposable evidence is incomplete or inconsistent."""


def object_value(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be an object")
    return value


def validate_report(report: object, planned: int, expected: tuple[str, int]) -> None:
    value = object_value(report, f"report {expected[0]}")
    counters = {
        "planned": planned,
        "would_upload": 0,
        "created": 0,
        "duplicate": 0,
        "resumed": 0,
        "retried": 0,
        "failed": 0,
        "indeterminate": 0,
    }
    dry_run = expected[0] == "would_upload"
    counters[expected[0]] = expected[1]
    required = {
        "schema_version": 1,
        "dry_run": dry_run,
        **counters,
        "cancelled": False,
    }
    if value != required:
        raise EvidenceError("disposable report counters drifted")


def validate(path: Path) -> None:
    try:
        evidence = object_value(json.loads(path.read_text(encoding="utf-8")), str(path))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot load {path}: {error}") from error
    schema = evidence.get("schema")
    if schema not in {"phase2-disposable-v1", "phase2-disposable-v2"}:
        raise EvidenceError("unsupported disposable evidence schema")
    if not isinstance(evidence.get("commit_sha"), str) or not COMMIT.fullmatch(evidence["commit_sha"]):
        raise EvidenceError("invalid implementation commit")
    environment = object_value(evidence.get("environment"), "environment")
    if environment.get("os") != "Linux" or environment.get("architecture") != "x86_64":
        raise EvidenceError("unexpected disposable platform")
    for field in ("binary_sha256",):
        if not isinstance(environment.get(field), str) or not SHA256.fullmatch(environment[field]):
            raise EvidenceError(f"invalid environment {field}")
    if evidence.get("images") != EXPECTED_IMAGES:
        raise EvidenceError("disposable image digest drift")
    fixture = object_value(evidence.get("fixture"), "fixture")
    if schema == "phase2-disposable-v1":
        if fixture.get("kind") != "synthetic" or fixture.get("license") != "CC0-1.0" or fixture.get("assets") != 1:
            raise EvidenceError("disposable fixture provenance drift")
        if not isinstance(fixture.get("sha256"), str) or not SHA256.fullmatch(fixture["sha256"]):
            raise EvidenceError("invalid disposable fixture digest")
        planned = 1
    else:
        validate_corpus(fixture)
        planned = 4
    plan_digest = evidence.get("plan_sha256")
    if not isinstance(plan_digest, str) or not SHA256.fullmatch(plan_digest):
        raise EvidenceError("invalid upload plan digest")
    methodology = object_value(evidence.get("methodology"), "methodology")
    if methodology.get("commands") != EXPECTED_COMMANDS or "127.0.0.1" not in methodology.get("network", ""):
        raise EvidenceError("disposable isolation methodology drift")
    if evidence.get("server_version") != {"major": 3, "minor": 1, "patch": 0, "prerelease": None}:
        raise EvidenceError("disposable server version drift")
    reports = object_value(evidence.get("reports"), "reports")
    validate_report(reports.get("dry_run"), planned, ("would_upload", planned))
    validate_report(reports.get("first"), planned, ("created", planned))
    validate_report(reports.get("resume"), planned, ("resumed", planned))
    validate_report(reports.get("duplicate"), planned, ("duplicate", planned))
    expected_visible = 1 if schema == "phase2-disposable-v1" else 3
    if evidence.get("asset_count") != expected_visible:
        raise EvidenceError("disposable visible asset count drift")
    if schema == "phase2-disposable-v2" and evidence.get("live_photo_links") != 1:
        raise EvidenceError("disposable live-photo relationship drift")
    cleanup = object_value(evidence.get("cleanup"), "cleanup")
    expected_cleanup = {
        "verified": True,
        "labelled_containers": 0,
        "labelled_volumes": 0,
        "labelled_networks": 0,
    }
    if any(cleanup.get(key) != value for key, value in expected_cleanup.items()):
        raise EvidenceError("disposable cleanup drift")
    removed = cleanup.get("downloaded_images_removed")
    if not isinstance(removed, int) or isinstance(removed, bool) or removed < 0 or removed > 3:
        raise EvidenceError("disposable image cleanup count drift")
    if schema == "phase2-disposable-v1" and removed != 3:
        raise EvidenceError("historical disposable cleanup drift")


def validate_corpus(fixture: dict[str, Any]) -> None:
    expected_roles = {
        "clip.mp4": "standalone-video",
        "image.jpg": "standalone-image",
        "image.xmp": "xmp-sidecar",
        "live.jpg": "live-photo-image",
        "live.mov": "live-photo-video",
    }
    if (
        fixture.get("schema") != "phase2-corpus-v1"
        or fixture.get("fixture_id") != "synthetic-phase2-media-matrix"
        or fixture.get("synthetic") is not True
        or fixture.get("license") != "CC0-1.0"
        or fixture.get("verified_materializations") != 2
        or fixture.get("expected") != {"upload_operations": 4, "xmp_sidecars": 1, "live_photo_pairs": 1}
    ):
        raise EvidenceError("Phase-2 corpus contract drift")
    generator = object_value(fixture.get("generator"), "fixture generator")
    if (
        generator.get("network") != "none"
        or generator.get("image") != EXPECTED_IMAGES["server"]
        or generator.get("recipe") != "ffmpeg-lavfi-apple-live-v2"
    ):
        raise EvidenceError("Phase-2 corpus provenance drift")
    files = fixture.get("files")
    if not isinstance(files, list) or [entry.get("path") for entry in files if isinstance(entry, dict)] != sorted(expected_roles):
        raise EvidenceError("Phase-2 corpus file matrix drift")
    for entry in files:
        value = object_value(entry, "fixture file")
        if value.get("role") != expected_roles.get(value.get("path")):
            raise EvidenceError("Phase-2 corpus role drift")
        if not isinstance(value.get("bytes"), int) or isinstance(value.get("bytes"), bool) or value["bytes"] <= 0:
            raise EvidenceError("Phase-2 corpus file length is invalid")
        if not isinstance(value.get("sha256"), str) or not SHA256.fullmatch(value["sha256"]):
            raise EvidenceError("Phase-2 corpus file digest is invalid")
    manifest_digest = fixture.get("manifest_sha256")
    if not isinstance(manifest_digest, str) or not SHA256.fullmatch(manifest_digest):
        raise EvidenceError("Phase-2 corpus manifest digest is invalid")


def main() -> int:
    paths = sorted(EVIDENCE_ROOT.glob("phase2-disposable-*.json"))
    matrix_paths = sorted(EVIDENCE_ROOT.glob("phase2-disposable-matrix-*.json"))
    try:
        if not paths:
            raise EvidenceError("Phase-2 disposable evidence is missing")
        if not matrix_paths:
            raise EvidenceError("Phase-2 disposable media-matrix evidence is missing")
        for path in paths:
            validate(path)
    except EvidenceError as error:
        print(f"disposable evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"disposable evidence passed: {len(paths)} report(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
