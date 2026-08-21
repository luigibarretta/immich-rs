"""Contract tests for the Apple Photos black-box comparator."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
EXPECTATION = REPOSITORY_ROOT / "tests/oracle/compatibility/apple-photos-v3.json"


def load_comparator():
    path = REPOSITORY_ROOT / "scripts/compare-apple-oracle.py"
    spec = importlib.util.spec_from_file_location("tested_apple_comparator", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_apple_comparator"] = module
    spec.loader.exec_module(module)
    return module


comparator = load_comparator()


def synthetic_observation() -> dict[str, object]:
    paths = [
        "Albums/Synthetic Journey/café.png",
        "Albums/Synthetic Journey/pair.mov",
        "Albums/Synthetic Journey/pair.png",
        "Albums/Synthetic Journey/rendered-edited.png",
        "Albums/Synthetic Journey/rendered.png",
    ]
    lines = []
    for index, path in enumerate(paths):
        archive = "icloud-001" if index < 3 else "icloud-002"
        kind = "video" if path.endswith(".mov") else "image"
        lines.append(f"INF discovered {kind} file={archive}:{path}")
        lines.append(f"INF uploaded successfully file={archive}:{path}")
    lines.extend(
        [
            "INF discovered sidecar file=icloud-002:Albums/Synthetic Journey/pair.xmp",
            "INF stacked file=icloud-001:Albums/Synthetic Journey/pair.mov",
            "INF stacked file=icloud-001:Albums/Synthetic Journey/pair.png",
            "WRN discovered banned file file=icloud-001:.DS_Store reason=banned file",
            "WRN discovered banned file file=icloud-002:Recently Deleted reason=banned folder",
        ]
    )
    return {
        "schema": "oracle-observation-v1",
        "case_id": "apple-photos-v3",
        "oracle": {"version": "0.32.0"},
        "fixture": {"archive_view": "icloud-split"},
        "process": {"exit_code": 0, "stdout": [], "logs": [{"lines": lines}]},
        "observable": {
            "all_requests_authenticated": True,
            "committed_mutations": [
                {"method": "PUT", "path": f"/api/jobs/{job}"}
                for job in (
                    "faceDetection",
                    "metadataExtraction",
                    "smartSearch",
                    "thumbnailGeneration",
                    "videoConversion",
                )
            ],
        },
    }


class AppleComparatorTests(unittest.TestCase):
    def compare(self, observation: dict[str, object]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observation.json"
            path.write_text(json.dumps(observation), encoding="utf-8")
            return comparator.compare(EXPECTATION, path)

    def test_declared_apple_matrix_passes(self) -> None:
        report = self.compare(synthetic_observation())
        self.assertTrue(all(check["passed"] for check in report["checks"]))
        self.assertEqual(report["counts"]["oracle_assets"], 5)

    def test_missing_variant_fails_closed(self) -> None:
        observation = synthetic_observation()
        logs = observation["process"]["logs"][0]["lines"]
        observation["process"]["logs"][0]["lines"] = [
            line for line in logs if "rendered-edited.png" not in line
        ]
        with self.assertRaises(comparator.DifferentialError):
            self.compare(observation)


if __name__ == "__main__":
    unittest.main()
