"""Tests for the portable ADR structure checker."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_adrs", ROOT / "scripts" / "check-adrs.py")
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load check-adrs.py")
CHECK_ADRS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK_ADRS
SPEC.loader.exec_module(CHECK_ADRS)


class AdrCheckTests(unittest.TestCase):
    def write_adr(self, root: Path, body: str) -> None:
        directory = root / "docs" / "adr"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "ADR-0001-test.md").write_text(body, encoding="utf-8")

    def test_accepts_complete_adr(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_adr(
                root,
                "# Test\n\n- Status: Accepted\n\n## Context\n\n## Decision\n\n"
                "## Consequences\n\n## Verification\n",
            )
            self.assertEqual(CHECK_ADRS.validate(root, 1), [])

    def test_rejects_invalid_status_and_missing_heading(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_adr(root, "# Test\n\n- Status: Draft\n\n## Context\n\n## Decision\n\n## Consequences\n")
            errors = CHECK_ADRS.validate(root, 1)
            self.assertTrue(any("missing valid status" in error for error in errors))
            self.assertTrue(any("missing ## Verification" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
