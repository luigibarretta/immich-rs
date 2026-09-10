"""Tests for fail-closed Web Console recovery evidence validation."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "docs/evidence/web-disposable-recovery-2026-09-10.json"


def load_checker():
    path = ROOT / "scripts/check-web-evidence.py"
    spec = importlib.util.spec_from_file_location("tested_web_evidence", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = load_checker()


class WebEvidenceTests(unittest.TestCase):
    def write_report(self, temporary: str, report: dict) -> Path:
        path = Path(temporary) / "report.json"
        path.write_text(json.dumps(report), encoding="utf-8")
        return path

    def test_committed_report_is_valid(self) -> None:
        checker.validate(EVIDENCE)

    def test_rejects_recovered_session(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["recovery"]["sessions_after_restart"] = 1
        with tempfile.TemporaryDirectory(prefix="immich-rs-web-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))

    def test_rejects_incomplete_cleanup(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["cleanup"]["processes_remaining"] = 1
        with tempfile.TemporaryDirectory(prefix="immich-rs-web-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))


if __name__ == "__main__":
    unittest.main()
