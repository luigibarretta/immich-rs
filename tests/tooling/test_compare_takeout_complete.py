"""Contract tests for the complete Google Takeout black-box comparator."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EXPECTATION = REPOSITORY_ROOT / "tests/oracle/compatibility/google-takeout-complete-v2.json"


def load_comparator():
    path = REPOSITORY_ROOT / "scripts" / "compare-takeout-complete.py"
    spec = importlib.util.spec_from_file_location("tested_complete_comparator", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_complete_comparator"] = module
    spec.loader.exec_module(module)
    return module


comparator = load_comparator()


def synthetic_observation() -> dict[str, object]:
    prefix = "Takeout/Google Photos/"
    alpha = prefix + "Photos from 2024/alpha.png"
    beta = prefix + "Photos from 2024/beta.png"
    cafe = prefix + "Photos from 2024/café.png"
    alias = prefix + "Synthetic Album/alpha.png"
    sidecars = [
        prefix + "Photos from 2024/alpha.png.json",
        prefix + "Photos from 2024/beta.png.supplemental-metadata.json",
        prefix + "Photos from 2024/café.png.json",
        prefix + "Synthetic Album/alpha.png.json",
        prefix + "Synthetic Album/metadata.json",
    ]
    lines = [
        *(f"INF discovered image file=takeout-001:{path}" for path in (alpha, beta, cafe)),
        f"INF discovered image file=takeout-002:{alias}",
        f"INF uploaded successfully file=takeout-001:{alpha}",
        f"INF uploaded successfully file=takeout-001:{cafe}",
        f"INF discarded local duplicate file=takeout-002:{alias} reason=local duplicate",
        f"INF metadata updated file=takeout-001:{alpha}",
        f"INF metadata updated file=takeout-001:{cafe}",
        "INF added to album :       3",
    ]
    for index, sidecar in enumerate(sidecars):
        kind = "album metadata" if sidecar.endswith("supplemental-metadata.json") else "asset metadata"
        archive = "takeout-001" if index < 3 else "takeout-002"
        lines.append(f"INF discovered sidecar file={archive}:{sidecar} type={kind} date=<TIMESTAMP>")
    return {
        "schema": "oracle-observation-v1",
        "case_id": "google-takeout-complete-v2",
        "oracle": {"version": "0.32.0"},
        "fixture": {"archive_view": "split"},
        "process": {"exit_code": 0, "stdout": [], "logs": [{"lines": lines}]},
        "observable": {
            "all_requests_authenticated": True,
            "committed_mutations": [{"method": "PUT", "path": "/api/jobs/faceDetection"}],
        },
    }


class CompleteTakeoutComparatorTests(unittest.TestCase):
    def compare(self, observation: dict[str, object]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observation.json"
            path.write_text(json.dumps(observation), encoding="utf-8")
            return comparator.compare(EXPECTATION, path)

    def test_declared_complete_matrix_passes(self) -> None:
        report = self.compare(synthetic_observation())
        self.assertTrue(all(check["passed"] for check in report["checks"]))

    def test_unclassified_supplemental_behavior_fails_closed(self) -> None:
        observation = synthetic_observation()
        lines = observation["process"]["logs"][0]["lines"]
        observation["process"]["logs"][0]["lines"] = [
            line for line in lines if "supplemental-metadata" not in line
        ]
        with self.assertRaises(comparator.DifferentialError):
            self.compare(observation)


if __name__ == "__main__":
    unittest.main()
