"""Tests for disk-backed bounded subprocess capture."""

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_capture():
    path = REPOSITORY_ROOT / "scripts" / "bounded-process.py"
    spec = importlib.util.spec_from_file_location("tested_bounded_process", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


capture = load_capture()


class BoundedProcessTests(unittest.TestCase):
    def test_captures_small_stdout_and_stderr(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-capture-") as temporary:
            result = capture.run_command(
                [sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr)"],
                Path(temporary),
                dict(os.environ),
                10,
            )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "out\n")
        self.assertEqual(result.stderr, "err\n")

    def test_rejects_output_over_limit(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-capture-limit-") as temporary:
            with self.assertRaises(capture.CaptureError):
                capture.run_command(
                    [sys.executable, "-c", f"print('x' * {capture.MAX_CAPTURE_BYTES})"],
                    Path(temporary),
                    dict(os.environ),
                    10,
                )


if __name__ == "__main__":
    unittest.main()
