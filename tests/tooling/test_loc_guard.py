import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
CHECKER = REPOSITORY / "scripts" / "check-loc.py"


class LocGuardTests(unittest.TestCase):
    def run_guard(self, source: str, max_lines: int = 4) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory(prefix="immich-rs-loc-") as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text(source, encoding="utf-8")
            policy = {
                "maxLines": max_lines,
                "allowedGrowthLines": 0,
                "roots": ["src"],
                "extensions": [".rs"],
                "ignore": {},
                "allow": {},
            }
            (root / "policy.json").write_text(json.dumps(policy), encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(CHECKER), "--root", str(root), "--policy", "policy.json"],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_accepts_file_at_limit(self) -> None:
        result = self.run_guard("one\ntwo\nthree\nfour\n")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_file_over_limit(self) -> None:
        result = self.run_guard("one\ntwo\nthree\nfour\nfive\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/lib.rs has 5 LOC (max 4)", result.stderr)


if __name__ == "__main__":
    unittest.main()
