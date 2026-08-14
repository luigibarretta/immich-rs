"""Black-box oracle runner tests with a fully synthetic fake executable."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import sys
import tempfile
import textwrap
import unittest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_runner():
    path = REPOSITORY_ROOT / "scripts" / "run-oracle.py"
    spec = importlib.util.spec_from_file_location("tested_oracle_runner", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["tested_oracle_runner"] = module
    spec.loader.exec_module(module)
    return module


runner = load_runner()


class OracleRunnerTests(unittest.TestCase):
    def test_version_and_digest_are_both_required(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "oracle"
            executable.write_text("#!/bin/sh\nprintf 'immich-go version 0.32.0\\n'\n", encoding="utf-8")
            executable.chmod(executable.stat().st_mode | stat.S_IXUSR)
            digest = hashlib.sha256(executable.read_bytes()).hexdigest()
            baseline = {
                "name": "immich-go",
                "version": "0.32.0",
                "upstream_commit": "synthetic-test-commit",
                "binary_sha256": digest,
            }
            verified = runner.verify_oracle(executable, baseline)
            self.assertEqual(verified["binary_sha256"], digest)
            baseline["binary_sha256"] = "0" * 64
            with self.assertRaises(runner.OracleError):
                runner.verify_oracle(executable, baseline)

    def test_full_run_is_synthetic_and_normalized(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture_root = root / "fixtures"
            fixture_dir = fixture_root / "synthetic-runner-test"
            fixture_dir.mkdir(parents=True)
            manifest = {
                "schema": "fixture-manifest-v1",
                "fixture_id": "synthetic-runner-test",
                "provenance": {
                    "kind": "synthetic",
                    "generator": "scripts/materialize-fixture.py",
                    "generator_version": "1",
                    "license": "CC0-1.0",
                },
                "files": [{"path": "pixel.png", "recipe": "synthetic_png", "width": 1, "height": 1, "rgb": [1, 2, 3]}],
                "expected_plan": {"schema": "normalized-plan-v1", "path": "expected-plan.json", "sha256": "0" * 64},
            }
            manifest_path = fixture_dir / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            case_path = fixture_dir / "case.json"
            case_path.write_text(
                json.dumps(
                    {
                        "schema": "oracle-case-v1",
                        "case_id": "synthetic-runner-test",
                        "fixture": "manifest.json",
                        "arguments": [
                            "upload",
                            "from-folder",
                            "--server",
                            "{server_url}",
                            "--api-key",
                            "{synthetic_api_key}",
                            "--dry-run",
                            "{fixture_root}",
                        ],
                        "timeout_seconds": 10,
                        "expected_oracle_mutations": [],
                    }
                ),
                encoding="utf-8",
            )
            executable = root / "fake-oracle"
            executable.write_text(
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import sys
                    import urllib.request
                    if "--version" in sys.argv:
                        print("immich-go version 0.32.0")
                        raise SystemExit(0)
                    server = sys.argv[sys.argv.index("--server") + 1]
                    api_key = sys.argv[sys.argv.index("--api-key") + 1]
                    fixture = sys.argv[-1]
                    request = urllib.request.Request(server + "/api/server/version", headers={"x-api-key": api_key})
                    with urllib.request.urlopen(request, timeout=2) as response:
                        response.read()
                    print(f"2026-01-02T03:04:05Z {fixture} 00000000-0000-4000-8000-000000000099")
                    """
                ),
                encoding="utf-8",
            )
            executable.chmod(executable.stat().st_mode | stat.S_IXUSR)
            digest = hashlib.sha256(executable.read_bytes()).hexdigest()
            baseline_path = root / "baseline.toml"
            baseline_path.write_text(
                textwrap.dedent(
                    f"""\
                    [oracle]
                    name = "immich-go"
                    version = "0.32.0"
                    upstream_commit = "synthetic-test-commit"

                    [artifacts.linux_x86_64]
                    binary_sha256 = "{digest}"
                    """
                ),
                encoding="utf-8",
            )
            original_fixture_root = runner.FIXTURE_ROOT
            runner.FIXTURE_ROOT = fixture_root.resolve()
            try:
                observation = runner.run_case(case_path, executable, baseline_path)
            finally:
                runner.FIXTURE_ROOT = original_fixture_root
            self.assertEqual(observation["schema"], "oracle-observation-v1")
            self.assertEqual(observation["observable"]["mutation_request_count"], 0)
            self.assertEqual(
                observation["process"]["stdout"],
                ["<TIMESTAMP> <FIXTURE_ROOT> <ID>"],
            )

    def test_case_rejects_embedded_server(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original_fixture_root = runner.FIXTURE_ROOT
            runner.FIXTURE_ROOT = root.resolve()
            manifest = root / "manifest.json"
            manifest.write_text("{}", encoding="utf-8")
            case = root / "case.json"
            case.write_text(
                json.dumps(
                    {
                        "schema": "oracle-case-v1",
                        "fixture": "manifest.json",
                        "case_id": "unsafe-server",
                        "arguments": [
                            "upload",
                            "from-folder",
                            "--server",
                            "{server_url}",
                            "--api-key",
                            "{synthetic_api_key}",
                            "--dry-run",
                            "https://example.invalid",
                            "{fixture_root}",
                        ],
                    }
                ),
                encoding="utf-8",
            )
            try:
                with self.assertRaises(runner.OracleError):
                    runner.load_case(case)
            finally:
                runner.FIXTURE_ROOT = original_fixture_root


if __name__ == "__main__":
    unittest.main()
