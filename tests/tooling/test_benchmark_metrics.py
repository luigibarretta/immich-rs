"""Smoke tests for bounded process metric collection."""

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_metrics():
    path = REPOSITORY_ROOT / "scripts" / "benchmark-metrics.py"
    spec = importlib.util.spec_from_file_location("tested_benchmark_metrics", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


metrics = load_metrics()


class BenchmarkMetricTests(unittest.TestCase):
    def test_collects_required_process_metrics(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-metrics-") as temporary:
            result, measured = metrics.run_command(
                [sys.executable, "-c", "print('synthetic benchmark process')"],
                Path(temporary),
                dict(os.environ),
                10,
            )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), "synthetic benchmark process")
        for field in (
            "wall_time_seconds",
            "user_cpu_seconds",
            "system_cpu_seconds",
            "peak_rss_bytes",
            "peak_open_file_descriptors",
            "characters_read",
            "characters_written",
            "storage_bytes_read",
            "storage_bytes_written",
        ):
            self.assertIn(field, measured)
            self.assertGreaterEqual(measured[field], 0)


if __name__ == "__main__":
    unittest.main()
