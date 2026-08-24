#!/usr/bin/env python3
"""Fail closed on unsafe, undeclared or non-reproducible test fixtures."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys
from types import ModuleType

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_ROOTS = [
    REPOSITORY_ROOT / "tests" / "fixtures" / "v1",
    REPOSITORY_ROOT / "tests" / "fixtures" / "v2",
    REPOSITORY_ROOT / "tests" / "fixtures" / "v3",
    REPOSITORY_ROOT / "tests" / "fixtures" / "v4",
]
SERVER_FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "oracle" / "server-fixtures"
SKIPPED_PARTS = {".git", "target", ".cargo"}
SELF_TEST_PATHS = {
    "scripts/check-fixtures.py",
    "tests/tooling/test_fixture_tools.py",
    "tests/tooling/test_mock_immich_server.py",
}
TEXT_SUFFIXES = {".json", ".md", ".py", ".rs", ".sh", ".toml", ".yml", ".yaml"}
SECRET_PATTERNS = {
    "credential assignment": re.compile(
        r"(?i)(?:api[_-]?key|x-api-key|authorization|bearer|password|secret)"
        r"\s*[:=]\s*[\"']?(?!synthetic|placeholder|redacted)[A-Za-z0-9_./+:-]{16,}"
    ),
    "AWS access key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "JWT": re.compile(r"\beyJ[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\b"),
}
PRODUCTION_PATTERNS = {
    "production hostname": re.compile(r"(?i)\bit[0-9]+-prd-[a-z0-9-]+\b"),
    "RFC1918 IPv4": re.compile(
        r"(?<![0-9])(?:10\.(?:[0-9]{1,3}\.){2}[0-9]{1,3}|192\.168\.(?:[0-9]{1,3}\.)[0-9]{1,3}|172\.(?:1[6-9]|2[0-9]|3[01])\.(?:[0-9]{1,3}\.)[0-9]{1,3})(?![0-9])"
    ),
}
PERSONAL_METADATA_KEYS = {
    "address",
    "altitude",
    "face",
    "faces",
    "gps",
    "latitude",
    "location",
    "longitude",
    "people",
    "person",
}


class CheckFailure(ValueError):
    """One fail-closed fixture or repository check failed."""


def _load_materializer() -> ModuleType:
    path = REPOSITORY_ROOT / "scripts" / "materialize-fixture.py"
    spec = importlib.util.spec_from_file_location("fixture_materializer", path)
    if spec is None or spec.loader is None:
        raise CheckFailure("cannot load fixture materializer")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _iter_repository_text() -> list[Path]:
    return sorted(
        path
        for path in REPOSITORY_ROOT.rglob("*")
        if path.is_file()
        and path.suffix.lower() in TEXT_SUFFIXES
        and not any(part in SKIPPED_PARTS for part in path.relative_to(REPOSITORY_ROOT).parts)
        and path.relative_to(REPOSITORY_ROOT).as_posix() not in SELF_TEST_PATHS
    )


def check_repository_secrets() -> None:
    """Reject credential-shaped data and known production infrastructure."""
    findings: list[str] = []
    for path in _iter_repository_text():
        relative = path.relative_to(REPOSITORY_ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        for label, pattern in {**SECRET_PATTERNS, **PRODUCTION_PATTERNS}.items():
            if pattern.search(text):
                findings.append(f"{relative}: contains {label}")
    if findings:
        raise CheckFailure("\n".join(findings))


def _walk_json(value: object, path: str = "$") -> list[str]:
    findings: list[str] = []
    if isinstance(value, dict):
        for key, child in value.items():
            if key.casefold() in PERSONAL_METADATA_KEYS:
                findings.append(f"{path}.{key}: personal metadata key is forbidden")
            findings.extend(_walk_json(child, f"{path}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            findings.extend(_walk_json(child, f"{path}[{index}]"))
    return findings


def check_manifests() -> None:
    """Validate every fixture and expected normalized-plan digest."""
    materializer = _load_materializer()
    manifests = sorted(
        manifest
        for root in FIXTURE_ROOTS
        if root.exists()
        for manifest in root.glob("*/manifest.json")
    )
    for manifest_path in manifests:
        try:
            manifest = materializer.load_manifest(manifest_path)
        except ValueError as error:
            raise CheckFailure(f"{manifest_path}: {error}") from error
        findings = _walk_json(manifest)
        if findings:
            raise CheckFailure("\n".join(f"{manifest_path}: {finding}" for finding in findings))
        expected = manifest.get("expected_plan")
        schema_versions = {
            "fixture-manifest-v1": 1,
            "fixture-manifest-v2": 2,
            "fixture-manifest-v3": 3,
            "fixture-manifest-v4": 4,
        }
        schema_version = schema_versions.get(manifest.get("schema"))
        if (
            not isinstance(expected, dict)
            or expected.get("schema") != f"normalized-plan-v{schema_version}"
        ):
            raise CheckFailure(f"{manifest_path}: invalid expected-plan declaration")
        expected_path_value = expected.get("path")
        expected_digest = expected.get("sha256")
        if expected_path_value != "expected-plan.json" or not isinstance(expected_digest, str):
            raise CheckFailure(f"{manifest_path}: expected plan path or digest is invalid")
        expected_path = manifest_path.parent / expected_path_value
        try:
            expected_bytes = expected_path.read_bytes()
            expected_json = json.loads(expected_bytes)
        except (OSError, json.JSONDecodeError) as error:
            raise CheckFailure(f"{expected_path}: cannot read expected plan: {error}") from error
        if expected_json.get("schema_version") != schema_version:
            raise CheckFailure(
                f"{expected_path}: normalized plan schema must be {schema_version}"
            )
        actual_digest = hashlib.sha256(expected_bytes).hexdigest()
        if actual_digest != expected_digest:
            raise CheckFailure(f"{expected_path}: expected-plan digest mismatch")
        allowed = {"manifest.json", "expected-plan.json"}
        extras = sorted(path.name for path in manifest_path.parent.iterdir() if path.name not in allowed)
        if extras:
            raise CheckFailure(f"{manifest_path.parent}: undeclared committed fixture files: {extras}")


def check_server_fixtures() -> None:
    """Require versioned synthetic provenance for every mock response fixture."""
    paths = sorted(SERVER_FIXTURE_ROOT.glob("*.json")) if SERVER_FIXTURE_ROOT.exists() else []
    if not paths:
        raise CheckFailure("mock server response fixtures are missing")
    for path in paths:
        try:
            fixture = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError) as error:
            raise CheckFailure(f"{path}: cannot read mock response fixture: {error}") from error
        provenance = fixture.get("provenance")
        version = fixture.get("version")
        if (
            fixture.get("schema") != "mock-immich-responses-v1"
            or not isinstance(fixture.get("fixture_id"), str)
            or not isinstance(provenance, dict)
            or provenance.get("kind") != "synthetic"
            or provenance.get("license") != "CC0-1.0"
            or not isinstance(version, dict)
            or any(not isinstance(version.get(key), int) for key in ("major", "minor", "patch"))
        ):
            raise CheckFailure(f"{path}: invalid version or synthetic provenance")
        findings = _walk_json(fixture)
        if findings:
            raise CheckFailure("\n".join(f"{path}: {finding}" for finding in findings))


def main() -> int:
    try:
        check_repository_secrets()
        check_manifests()
        check_server_fixtures()
    except (CheckFailure, OSError, UnicodeError) as error:
        print(f"fixture safety check failed:\n{error}", file=sys.stderr)
        return 1
    print("fixture safety and provenance checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
