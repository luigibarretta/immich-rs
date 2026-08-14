"""Tests for fail-closed validation of committed benchmark evidence."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = REPOSITORY_ROOT / "benchmarks/evidence/phase1-2026-08-14.json"


def load_checker():
    path = REPOSITORY_ROOT / "scripts/check-benchmark-evidence.py"
    spec = importlib.util.spec_from_file_location("tested_benchmark_evidence", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = load_checker()


class BenchmarkEvidenceTests(unittest.TestCase):
    def write_report(self, temporary: str, report: dict) -> Path:
        path = Path(temporary) / "report.json"
        path.write_text(json.dumps(report), encoding="utf-8")
        return path

    def test_committed_evidence_is_self_consistent(self) -> None:
        checker.validate(EVIDENCE)

    def test_rejects_operation_count_drift(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["raw_samples"][0]["immich_rs"]["operations"]["http_requests"] = 1
        with tempfile.TemporaryDirectory(prefix="immich-rs-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))

    def test_rejects_aggregate_tampering(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["aggregate"]["immich_rs"]["wall_time_seconds"]["median"] = 0
        with tempfile.TemporaryDirectory(prefix="immich-rs-evidence-") as temporary:
            with self.assertRaises(checker.EvidenceError):
                checker.validate(self.write_report(temporary, report))


if __name__ == "__main__":
    unittest.main()
