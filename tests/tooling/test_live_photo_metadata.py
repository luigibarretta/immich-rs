"""Tests for bounded synthetic Apple live-photo JPEG metadata."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY_ROOT / "scripts" / "add-live-photo-metadata.py"


def load_module():
    spec = importlib.util.spec_from_file_location("tested_live_photo_metadata", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load live-photo metadata generator")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class LivePhotoMetadataTests(unittest.TestCase):
    def test_payload_contains_bounded_synthetic_identifier(self) -> None:
        module = load_module()
        payload = module.exif_payload("synthetic-live-photo-v1")
        self.assertTrue(payload.startswith(b"Exif\0\0MM\x00*"))
        self.assertIn(b"Apple\0", payload)
        self.assertIn(b"Apple iOS\0\0\x01MM", payload)
        self.assertIn(b"synthetic-live-photo-v1\0", payload)
        self.assertLess(len(payload), 65_533)

    def test_stream_injection_preserves_the_jpeg_body(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(SCRIPT), "--content-identifier", "synthetic-live-photo-v1"],
            input=b"\xff\xd8synthetic-jpeg-body",
            check=True,
            capture_output=True,
        )
        self.assertTrue(completed.stdout.startswith(b"\xff\xd8\xff\xe1"))
        self.assertTrue(completed.stdout.endswith(b"synthetic-jpeg-body"))

    def test_non_jpeg_input_fails_closed(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(SCRIPT), "--content-identifier", "synthetic-live-photo-v1"],
            input=b"not-a-jpeg",
            check=False,
            capture_output=True,
        )
        self.assertEqual(completed.returncode, 1)
        self.assertEqual(completed.stdout, b"")


if __name__ == "__main__":
    unittest.main()
