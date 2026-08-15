"""Contract tests for the Google Takeout black-box comparator."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EXPECTATION = (
    REPOSITORY_ROOT
    / "tests"
    / "oracle"
    / "compatibility"
    / "google-takeout-basic-v1.json"
)
PLAN = (
    REPOSITORY_ROOT
    / "tests"
    / "fixtures"
    / "v1"
    / "synthetic-google-takeout-basic"
    / "expected-plan.json"
)


def load_comparator():
    path = REPOSITORY_ROOT / "scripts" / "compare-takeout-oracle.py"
    spec = importlib.util.spec_from_file_location("tested_takeout_comparator", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_takeout_comparator"] = module
    spec.loader.exec_module(module)
    return module


comparator = load_comparator()


def synthetic_observation() -> dict[str, object]:
    plan = json.loads(PLAN.read_text(encoding="utf-8"))
    lines = []
    for asset in plan["assets"]:
        path = asset["relative_path"]
        lines.extend(
            [
                f"<TIMESTAMP> INF discovered image file=fixture:{path}",
                f"<TIMESTAMP> INF uploaded successfully file=fixture:{path}",
                f"<TIMESTAMP> INF metadata updated file=fixture:{path}",
            ]
        )
        for metadata in asset["metadata"]:
            lines.append(
                "<TIMESTAMP> INF discovered sidecar "
                f"file=fixture:{metadata['relative_path']} type=asset metadata"
            )
    return {
        "schema": "oracle-observation-v1",
        "case_id": "google-takeout-basic-v1",
        "oracle": {"version": "0.32.0"},
        "process": {"exit_code": 0, "logs": [{"name": "synthetic.log", "lines": lines}]},
        "observable": {
            "all_requests_authenticated": True,
            "committed_mutations": [{"method": "PUT", "path": "/api/jobs/faceDetection"}],
        },
    }


class TakeoutComparatorTests(unittest.TestCase):
    def compare(self, observation: dict[str, object]) -> dict[str, object]:
        with tempfile.TemporaryDirectory(prefix="immich-rs-takeout-differential-") as temporary:
            path = Path(temporary) / "observation.json"
            path.write_text(json.dumps(observation), encoding="utf-8")
            return comparator.compare(EXPECTATION, path)

    def test_exact_takeout_matrix_passes(self) -> None:
        report = self.compare(synthetic_observation())
        self.assertTrue(all(check["passed"] for check in report["checks"]))

    def test_missing_metadata_association_fails_closed(self) -> None:
        observation = synthetic_observation()
        lines = observation["process"]["logs"][0]["lines"]
        observation["process"]["logs"][0]["lines"] = [
            line for line in lines if "metadata updated" not in line
        ]
        with self.assertRaises(comparator.TakeoutDifferentialError):
            self.compare(observation)


if __name__ == "__main__":
    unittest.main()
