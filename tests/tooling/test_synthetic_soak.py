"""Tests for the bounded deterministic synthetic soak generator."""

from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "synthetic_soak", ROOT / "scripts/run-synthetic-soak.py"
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load synthetic soak module")
SOAK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SOAK
SPEC.loader.exec_module(SOAK)


class SyntheticSoakTests(unittest.TestCase):
    def test_materialization_is_reproducible_and_fully_allocated(self) -> None:
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            left = SOAK.materialize(Path(first), 2, SOAK.BUFFER_BYTES)
            right = SOAK.materialize(Path(second), 2, SOAK.BUFFER_BYTES)
            self.assertEqual(left, right)
            self.assertEqual(left["assets"], 2)
            self.assertEqual(
                left["logical_source_bytes"],
                left["logical_media_bytes"] + left["logical_sidecar_bytes"],
            )
            self.assertGreaterEqual(left["allocated_bytes"], left["logical_source_bytes"])

    def test_asset_content_is_unique_and_bounded(self) -> None:
        first = SOAK.media_block(0)
        second = SOAK.media_block(1)
        self.assertEqual(len(first), SOAK.BUFFER_BYTES)
        self.assertNotEqual(hashlib.sha256(first).digest(), hashlib.sha256(second).digest())


if __name__ == "__main__":
    unittest.main()
