"""The architecture check reads push-gate commands from the ci-local manifest."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "check_architecture", REPOSITORY_ROOT / "scripts" / "check-architecture.py"
)
assert SPEC is not None and SPEC.loader is not None
architecture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(architecture)

COMMAND = "python3 scripts/check-production-evidence.py"


class PushGateTests(unittest.TestCase):
    def gate(self, run: object, only: str = "evidence") -> bool:
        with tempfile.TemporaryDirectory(prefix="immich-rs-push-gate-") as temporary:
            root = Path(temporary)
            (root / ".gitea" / "workflows").mkdir(parents=True)
            (root / ".gitea" / "workflows" / "ci.yml").write_text(
                "jobs:\n  quality:\n    steps:\n"
                "      # node third_party/repo-tooling/ci-local.mjs --ci --only commented\n"
                f"      - run: node third_party/repo-tooling/ci-local.mjs --ci --only {only}\n",
                encoding="utf-8",
            )
            manifest = {"steps": [{"name": "evidence", "run": run}, {"name": "commented", "run": run}]}
            (root / "ci-local.json").write_text(json.dumps(manifest), encoding="utf-8")
            original = architecture.REPOSITORY_ROOT
            architecture.REPOSITORY_ROOT = root
            try:
                return architecture.push_gate_runs(COMMAND)
            finally:
                architecture.REPOSITORY_ROOT = original

    def test_the_repository_push_gate_enforces_production_evidence(self) -> None:
        self.assertTrue(architecture.push_gate_runs(COMMAND))

    def test_a_chained_or_argv_step_owned_by_a_job_counts(self) -> None:
        self.assertTrue(self.gate(f"python3 scripts/check-adrs.py && {COMMAND}"))
        self.assertTrue(self.gate(COMMAND.split()))

    def test_an_unowned_missing_or_partial_command_does_not_count(self) -> None:
        self.assertFalse(self.gate(COMMAND, only="other"))
        self.assertFalse(self.gate("python3 scripts/check-web-evidence.py"))
        self.assertFalse(self.gate(f"{COMMAND} --skip"))


if __name__ == "__main__":
    unittest.main()
