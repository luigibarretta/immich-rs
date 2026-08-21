"""Contract tests for committed Phase 4 benchmark evidence."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = REPOSITORY_ROOT / "benchmarks/evidence/phase4-2026-08-21.json"


def load_checker():
    path = REPOSITORY_ROOT / "scripts/check-phase4-benchmark.py"
    spec = importlib.util.spec_from_file_location("tested_phase4_evidence", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_phase4_evidence"] = module
    spec.loader.exec_module(module)
    return module


checker = load_checker()


class Phase4EvidenceTests(unittest.TestCase):
    def test_committed_evidence_is_valid(self) -> None:
        checker.validate(EVIDENCE)

    def test_network_capability_drift_fails_closed(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        changed = copy.deepcopy(report)
        changed["raw_samples"][0]["immich_rs"]["operations"]["http_requests"] = 1
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "changed.json"
            path.write_text(json.dumps(changed), encoding="utf-8")
            with self.assertRaises(checker.EvidenceError):
                checker.validate(path)

    def test_aggregate_drift_fails_closed(self) -> None:
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        report["aggregate"]["immich_go"]["wall_time_seconds"]["median"] = 0
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "changed.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaises(checker.EvidenceError):
                checker.validate(path)


if __name__ == "__main__":
    unittest.main()
