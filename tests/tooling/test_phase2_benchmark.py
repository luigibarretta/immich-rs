"""Contract tests for the isolated Phase-2 benchmark driver."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    path = REPOSITORY_ROOT / "scripts" / "benchmark-phase2.py"
    spec = importlib.util.spec_from_file_location("tested_phase2_benchmark", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load Phase-2 benchmark driver")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class Phase2BenchmarkTests(unittest.TestCase):
    def test_failure_diagnostics_redact_every_sensitive_value(self) -> None:
        module = load_module()
        completed = subprocess.CompletedProcess([], 1, "key at endpoint", "source in workspace")
        diagnostic = module.redacted_failure(completed, ["key", "endpoint", "source", "workspace"])
        self.assertEqual(diagnostic, "<REDACTED> at <REDACTED>\n<REDACTED> in <REDACTED>")

    def test_combine_sums_work_and_keeps_resource_peaks(self) -> None:
        module = load_module()
        first = {field: 2 for field in module.METRICS}
        second = {field: 3 for field in module.METRICS}
        combined = module.combine([first, second])
        for field in module.SUM_METRICS:
            self.assertEqual(combined[field], 5)
        self.assertEqual(combined["peak_rss_bytes"], 3)
        self.assertEqual(combined["peak_open_file_descriptors"], 3)

    def test_environment_is_allowlisted_and_secret_is_explicit(self) -> None:
        module = load_module()
        previous = os.environ.get("IMMICH_RS_BENCHMARK_ADMIN_TOKEN")
        os.environ["IMMICH_RS_BENCHMARK_ADMIN_TOKEN"] = "must-not-be-inherited"
        try:
            with tempfile.TemporaryDirectory() as temporary:
                environment = module.safe_environment(Path(temporary), "synthetic-key")
        finally:
            if previous is None:
                os.environ.pop("IMMICH_RS_BENCHMARK_ADMIN_TOKEN", None)
            else:
                os.environ["IMMICH_RS_BENCHMARK_ADMIN_TOKEN"] = previous
        self.assertEqual(environment["IMMICH_RS_API_KEY"], "synthetic-key")
        self.assertNotIn("IMMICH_RS_BENCHMARK_ADMIN_TOKEN", environment)
        self.assertEqual(set(environment), {"HOME", "LANG", "LC_ALL", "PATH", "TZ", "IMMICH_RS_API_KEY"})

    def test_tool_inspection_does_not_inherit_benchmark_credentials(self) -> None:
        module = load_module()
        previous = os.environ.get("IMMICH_RS_BENCHMARK_ADMIN_TOKEN")
        os.environ["IMMICH_RS_BENCHMARK_ADMIN_TOKEN"] = "synthetic-admin-token"
        try:
            with tempfile.TemporaryDirectory() as temporary:
                output = module.command_output(
                    [
                        sys.executable,
                        "-c",
                        "import os; print('IMMICH_RS_BENCHMARK_ADMIN_TOKEN' in os.environ)",
                    ],
                    Path(temporary),
                )
        finally:
            if previous is None:
                os.environ.pop("IMMICH_RS_BENCHMARK_ADMIN_TOKEN", None)
            else:
                os.environ["IMMICH_RS_BENCHMARK_ADMIN_TOKEN"] = previous
        self.assertEqual(output, "False")


if __name__ == "__main__":
    unittest.main()
