"""Tests for bounded synthetic media identifier rewriting."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY_ROOT / "scripts" / "rewrite-synthetic-identifier.py"
SOURCE = "synthetic-live-photo-v1"
TARGET = "synthetic-still-only-v1"


class RewriteSyntheticIdentifierTests(unittest.TestCase):
    def run_rewrite(self, payload: bytes, target: str = TARGET) -> subprocess.CompletedProcess[bytes]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--from-identifier", SOURCE, "--to-identifier", target],
            input=payload,
            capture_output=True,
            check=False,
        )

    def test_rewrites_one_identifier_across_chunk_boundary(self) -> None:
        prefix = b"x" * 65_530
        completed = self.run_rewrite(prefix + SOURCE.encode() + b"tail")
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(completed.stdout, prefix + TARGET.encode() + b"tail")

    def test_rejects_missing_or_multiple_identifiers(self) -> None:
        self.assertNotEqual(self.run_rewrite(b"synthetic media").returncode, 0)
        repeated = SOURCE.encode() + SOURCE.encode()
        self.assertNotEqual(self.run_rewrite(repeated).returncode, 0)

    def test_rejects_length_changing_replacement(self) -> None:
        self.assertNotEqual(self.run_rewrite(SOURCE.encode(), "short").returncode, 0)


if __name__ == "__main__":
    unittest.main()
