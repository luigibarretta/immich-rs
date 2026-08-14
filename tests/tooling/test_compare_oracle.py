"""Contract tests for normalized black-box differential comparison."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EXPECTATION = REPOSITORY_ROOT / "tests" / "oracle" / "compatibility" / "folder-matrix-v1.json"
PLAN = REPOSITORY_ROOT / "tests" / "fixtures" / "v1" / "synthetic-folder-matrix" / "expected-plan.json"


def load_comparator():
    path = REPOSITORY_ROOT / "scripts" / "compare-oracle.py"
    spec = importlib.util.spec_from_file_location("tested_oracle_comparator", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_oracle_comparator"] = module
    spec.loader.exec_module(module)
    return module


comparator = load_comparator()


def synthetic_observation() -> dict[str, object]:
    plan = json.loads(PLAN.read_text(encoding="utf-8"))
    assets = {asset["relative_path"]: asset for asset in plan["assets"]}
    lines = []
    for path, asset in sorted(assets.items()):
        lines.append(f"<TIMESTAMP> INF discovered {asset['media_kind']} file=fixture:{path}")
        lines.append(f"<TIMESTAMP> INF uploaded successfully file=fixture:{path}")
        if asset["live_photo"] is not None:
            lines.append(f"<TIMESTAMP> INF stacked file=fixture:{path}")
        for metadata in asset["metadata"]:
            lines.append(f"<TIMESTAMP> INF discovered sidecar file=fixture:{metadata['relative_path']}")
    lines.extend(
        [
            "<TIMESTAMP> INF discovered image file=fixture:link.png",
            "<TIMESTAMP> WRN discarded local duplicate file=fixture:link.png",
            "<TIMESTAMP> INF discovered sidecar file=fixture:unmatched.json",
            "<TIMESTAMP> WRN JSON file detected but not from immich-go file=fixture:alpha.png.json",
        ]
    )
    return {
        "schema": "oracle-observation-v1",
        "case_id": "folder-matrix-v1",
        "oracle": {"version": "0.32.0"},
        "process": {"exit_code": 0, "logs": [{"name": "synthetic.log", "lines": lines}]},
        "observable": {
            "all_requests_authenticated": True,
            "committed_mutations": [{"method": "PUT", "path": "/api/jobs/faceDetection"}],
        },
    }


class OracleComparatorTests(unittest.TestCase):
    def compare(self, observation: dict[str, object]) -> dict[str, object]:
        with tempfile.TemporaryDirectory(prefix="immich-rs-differential-") as temporary:
            path = Path(temporary) / "observation.json"
            path.write_text(json.dumps(observation), encoding="utf-8")
            return comparator.compare(EXPECTATION, path)

    def test_declared_matrix_passes_with_exact_normalized_facts(self) -> None:
        report = self.compare(synthetic_observation())
        self.assertTrue(all(check["passed"] for check in report["checks"]))

    def test_missing_oracle_asset_fails_closed(self) -> None:
        observation = synthetic_observation()
        lines = observation["process"]["logs"][0]["lines"]
        observation["process"]["logs"][0]["lines"] = [
            line for line in lines if "uploaded successfully file=fixture:alpha.png" not in line
        ]
        with self.assertRaises(comparator.DifferentialError):
            self.compare(observation)


if __name__ == "__main__":
    unittest.main()
