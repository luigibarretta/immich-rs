"""Tests for fixture safety and deterministic materialization."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_script(name: str):
    path = REPOSITORY_ROOT / "scripts" / name
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


materializer = load_script("materialize-fixture.py")
checker = load_script("check-fixtures.py")


class FixtureToolTests(unittest.TestCase):
    def manifest(self) -> dict[str, object]:
        return {
            "schema": "fixture-manifest-v1",
            "fixture_id": "synthetic-tool-test",
            "provenance": {
                "kind": "synthetic",
                "generator": "scripts/materialize-fixture.py",
                "generator_version": "1",
                "license": "CC0-1.0",
            },
            "files": [
                {
                    "path": "image.png",
                    "recipe": "synthetic_png",
                    "width": 2,
                    "height": 1,
                    "rgb": [17, 34, 51],
                }
            ],
            "expected_plan": {
                "schema": "normalized-plan-v1",
                "path": "expected-plan.json",
                "sha256": "0" * 64,
            },
        }

    def test_materialization_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps(self.manifest()), encoding="utf-8")
            first = root / "first"
            second = root / "second"
            materializer.materialize(manifest_path, first)
            materializer.materialize(manifest_path, second)
            self.assertEqual((first / "image.png").read_bytes(), (second / "image.png").read_bytes())
            self.assertTrue((first / "image.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))

    def test_manifest_rejects_traversal(self) -> None:
        manifest = self.manifest()
        manifest["files"][0]["path"] = "../escape.png"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "manifest.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaises(materializer.FixtureError):
                materializer.load_manifest(path)

    def test_personal_metadata_keys_are_rejected(self) -> None:
        findings = checker._walk_json({"metadata": {"latitude": 1}})
        self.assertTrue(findings)

    def test_credential_and_production_canaries_are_detected(self) -> None:
        self.assertTrue(checker.SECRET_PATTERNS["AWS access key"].search("AKIAABCDEFGHIJKLMNOP"))
        self.assertTrue(checker.PRODUCTION_PATTERNS["production hostname"].search("it1-prd-photo-01"))
        self.assertTrue(checker.PRODUCTION_PATTERNS["RFC1918 IPv4"].search("192.168.50.10"))


if __name__ == "__main__":
    unittest.main()
