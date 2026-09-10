"""Tests for fail-closed validation of Phase 8 benchmark evidence."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = REPOSITORY_ROOT / "benchmarks/evidence/phase8-2026-08-24.json"


def load_checker():
    path = REPOSITORY_ROOT / "scripts/check-phase8-benchmark.py"
    spec = importlib.util.spec_from_file_location("tested_phase8_benchmark", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = load_checker()


class Phase8BenchmarkEvidenceTests(unittest.TestCase):
    def write_report(self, temporary: str, report: dict) -> Path:
        path = Path(temporary) / "report.json"
        path.write_text(json.dumps(report), encoding="utf-8")
        return path

    def test_committed_evidence_is_self_consistent(self) -> None:
        checker.validate(EVIDENCE)

    def test_accepts_release_candidate_version(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["manifest"]["tools"]["immich_rs"]["version"] = "immich-rs 0.1.0-rc.1"
        with tempfile.TemporaryDirectory(prefix="immich-rs-phase8-evidence-") as temporary:
            checker.validate(self.write_report(temporary, report))

    def test_rejects_unbounded_prerelease_identity(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["manifest"]["tools"]["immich_rs"]["version"] = "immich-rs latest"
        with tempfile.TemporaryDirectory(prefix="immich-rs-phase8-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))

    def test_rejects_zero_release_candidate(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["manifest"]["tools"]["immich_rs"]["version"] = "immich-rs 0.1.0-rc.0"
        with tempfile.TemporaryDirectory(prefix="immich-rs-phase8-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))


if __name__ == "__main__":
    unittest.main()
