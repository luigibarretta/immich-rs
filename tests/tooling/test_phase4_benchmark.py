"""Contract tests for the paired Phase 4 benchmark driver."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_driver():
    path = REPOSITORY_ROOT / "scripts/benchmark-phase4.py"
    spec = importlib.util.spec_from_file_location("tested_phase4_benchmark", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_phase4_benchmark"] = module
    spec.loader.exec_module(module)
    return module


driver = load_driver()


class Phase4BenchmarkTests(unittest.TestCase):
    def test_oracle_paths_are_normalized_across_split_archives(self) -> None:
        observation = {
            "process": {
                "logs": [
                    {
                        "lines": [
                            "INF uploaded successfully file=icloud-002:Album/beta.png",
                            "INF uploaded successfully file=icloud-001:Album/alpha.png",
                        ]
                    }
                ]
            }
        }
        self.assertEqual(
            driver._oracle_paths(observation, "uploaded successfully"),
            {"Album/alpha.png", "Album/beta.png"},
        )

    def test_sample_bounds_fail_before_process_execution(self) -> None:
        with self.assertRaises(driver.Phase4BenchmarkError):
            driver.run(1, 0, Path("missing-oracle"), Path("missing-rust"))

    def test_metric_contract_includes_resource_and_logical_io(self) -> None:
        self.assertIn("peak_rss_bytes", driver.METRIC_FIELDS)
        self.assertIn("peak_open_file_descriptors", driver.METRIC_FIELDS)
        self.assertIn("logical_media_bytes_read", driver.METRIC_FIELDS)


if __name__ == "__main__":
    unittest.main()
