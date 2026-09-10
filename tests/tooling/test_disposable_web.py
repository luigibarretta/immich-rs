"""Tests for the disposable Web Console recovery rehearsal."""

import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "run-disposable-web.py"


def load_script():
    spec = importlib.util.spec_from_file_location("tested_disposable_web", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


rehearsal = load_script()


class DisposableWebTests(unittest.TestCase):
    def test_tree_digest_is_copy_stable_and_content_sensitive(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-web-tooling.") as temporary:
            root = Path(temporary)
            source = root / "source"
            copied = root / "copied"
            source.mkdir()
            (source / "state.sqlite3").write_bytes(b"synthetic state\n")
            shutil.copytree(source, copied)
            self.assertEqual(rehearsal.tree_digest(source), rehearsal.tree_digest(copied))
            (copied / "state.sqlite3").write_bytes(b"changed synthetic state\n")
            self.assertNotEqual(rehearsal.tree_digest(source), rehearsal.tree_digest(copied))

    def test_help_and_isolation_contract_are_explicit(self) -> None:
        completed = subprocess.run(
            ["python3", str(SCRIPT), "--help"],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("--commit-sha", completed.stdout)
        text = SCRIPT.read_text(encoding="utf-8")
        for required in (
            "127.0.0.1/32",
            "TemporaryDirectory",
            "process.send_signal(signal.SIGINT)",
            '"production_access": False',
            '"copied_while_stopped": True',
        ):
            self.assertIn(required, text)
        for forbidden in ("0.0.0.0", "IMMICH_RS_API_KEY", "authentik"):
            self.assertNotIn(forbidden, text)


if __name__ == "__main__":
    unittest.main()
