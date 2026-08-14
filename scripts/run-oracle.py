#!/usr/bin/env python3
"""Run the pinned immich-go oracle only against declared synthetic fixtures."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib
from types import ModuleType
from typing import Any
import unicodedata

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
BASELINE_PATH = REPOSITORY_ROOT / "tests" / "oracle" / "baseline.toml"
FIXTURE_ROOT = (REPOSITORY_ROOT / "tests" / "fixtures" / "v1").resolve()
ORACLE_OBSERVATION_SCHEMA = "oracle-observation-v1"
CASE_SCHEMA = "oracle-case-v1"
ANSI_ESCAPE = re.compile(r"\x1b(?:[@-_][0-?]*[ -/]*[@-~]|\[[0-?]*[ -/]*[@-~])")
UUID = re.compile(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b")
RFC3339 = re.compile(r"\b\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+Z?\b")
CLOCK_TIME = re.compile(r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b")
LOG_TIMESTAMP = re.compile(r"(?<!\d)\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}(?!\d)")
COORDINATE_FIELDS = re.compile(r"\s+file\.(?:Latitude|Longitude)=[^\s]+")
LOGGED_API_KEY = re.compile(r"--api-key=[^\s]+ origin=cli")


class OracleError(ValueError):
    """The oracle baseline, executable, fixture or case is invalid."""


def _load_python(path: Path, module_name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise OracleError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def load_baseline(path: Path = BASELINE_PATH) -> dict[str, Any]:
    """Load the pinned oracle version and current-platform digest."""
    try:
        baseline = tomllib.loads(path.read_text(encoding="utf-8"))
        oracle = baseline["oracle"]
        artifact = baseline["artifacts"]["linux_x86_64"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise OracleError(f"invalid oracle baseline: {error}") from error
    required = {
        "name": oracle.get("name"),
        "version": oracle.get("version"),
        "upstream_commit": oracle.get("upstream_commit"),
        "binary_sha256": artifact.get("binary_sha256"),
    }
    if not all(isinstance(value, str) and value for value in required.values()):
        raise OracleError("oracle baseline is missing required strings")
    return required


def verify_oracle(executable: Path, baseline: dict[str, Any]) -> dict[str, str]:
    """Verify executable digest and reported version before any fixture runs."""
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise OracleError(f"oracle is not executable: {executable}")
    actual_digest = _sha256_file(executable)
    if actual_digest != baseline["binary_sha256"]:
        raise OracleError("oracle executable digest mismatch")
    try:
        result = subprocess.run(
            [str(executable), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise OracleError(f"cannot read oracle version: {error}") from error
    combined = f"{result.stdout}\n{result.stderr}"
    if result.returncode != 0 or baseline["version"] not in combined:
        raise OracleError("oracle reported an unexpected version")
    return {
        "name": baseline["name"],
        "version": baseline["version"],
        "upstream_commit": baseline["upstream_commit"],
        "binary_sha256": actual_digest,
    }


def _safe_fixture_manifest(case_path: Path, raw_path: object) -> Path:
    if not isinstance(raw_path, str):
        raise OracleError("oracle case fixture must be a string")
    manifest = (case_path.parent / raw_path).resolve()
    if FIXTURE_ROOT not in manifest.parents or manifest.name != "manifest.json":
        raise OracleError("oracle fixture must be a v1 manifest inside tests/fixtures/v1")
    return manifest


def load_case(path: Path) -> dict[str, Any]:
    """Load a bounded oracle case without accepting arbitrary server targets."""
    try:
        case = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OracleError(f"cannot read oracle case: {error}") from error
    if not isinstance(case, dict) or case.get("schema") != CASE_SCHEMA:
        raise OracleError(f"oracle case schema must be {CASE_SCHEMA}")
    if not isinstance(case.get("case_id"), str) or not case["case_id"]:
        raise OracleError("oracle case_id must be a non-empty string")
    arguments = case.get("arguments")
    if not isinstance(arguments, list) or not arguments or not all(isinstance(value, str) for value in arguments):
        raise OracleError("oracle case arguments must be a non-empty string array")
    if arguments[:2] != ["upload", "from-folder"] or "--dry-run" not in arguments:
        raise OracleError("oracle case must be a dry-run folder upload")
    required_placeholders = {"{server_url}", "{fixture_root}", "{synthetic_api_key}"}
    if any(arguments.count(placeholder) != 1 for placeholder in required_placeholders):
        raise OracleError("oracle case must use each synthetic placeholder exactly once")
    if any(re.search(r"(?i)https?://", argument) for argument in arguments):
        raise OracleError("oracle case cannot embed a server URL")
    forbidden_flags = {"--admin-api-key", "--config", "--overwrite", "--save-config", "--session-tag"}
    if any(argument in forbidden_flags for argument in arguments):
        raise OracleError("oracle case contains a forbidden stateful flag")
    for index, argument in enumerate(arguments):
        if argument in {"--server", "-s"}:
            if index + 1 >= len(arguments) or arguments[index + 1] != "{server_url}":
                raise OracleError("oracle server flag must use the mock placeholder")
        if argument in {"--api-key", "-k"}:
            if index + 1 >= len(arguments) or arguments[index + 1] != "{synthetic_api_key}":
                raise OracleError("oracle API key flag must use the synthetic placeholder")
        if argument.startswith(("--server=", "--api-key=")):
            raise OracleError("oracle server and API key must use separate placeholders")
    if arguments.count("--server") != 1 or arguments.count("--api-key") != 1:
        raise OracleError("oracle case must declare one mock server and synthetic API key")
    fixture = _safe_fixture_manifest(path.resolve(), case.get("fixture"))
    timeout_seconds = case.get("timeout_seconds", 60)
    if not isinstance(timeout_seconds, int) or not 1 <= timeout_seconds <= 300:
        raise OracleError("oracle timeout must be in 1..300 seconds")
    expected_mutations = case.get("expected_oracle_mutations", [])
    if not isinstance(expected_mutations, list) or any(
        not isinstance(item, dict)
        or set(item) != {"method", "path", "json_body", "classification"}
        or item.get("classification") != "oracle_defect"
        for item in expected_mutations
    ):
        raise OracleError("expected oracle mutations must be explicit oracle-defect records")
    return {**case, "fixture_path": fixture, "timeout_seconds": timeout_seconds}


def _normalize_text(text: str, fixture_root: Path, workspace_root: Path, server_url: str) -> list[str]:
    normalized = ANSI_ESCAPE.sub("", text).replace(str(fixture_root), "<FIXTURE_ROOT>")
    normalized = normalized.replace(str(workspace_root), "<WORKSPACE>").replace(server_url, "<MOCK_SERVER>")
    normalized = UUID.sub("<ID>", normalized)
    normalized = RFC3339.sub("<TIMESTAMP>", normalized)
    normalized = CLOCK_TIME.sub("<TIME>", normalized)
    normalized = LOG_TIMESTAMP.sub("<TIMESTAMP>", normalized)
    normalized = normalized.replace("synthetic-oracle-key", "<SYNTHETIC_API_KEY>")
    normalized = normalized.replace("synthetic-user@example.invalid", "<SYNTHETIC_USER>")
    normalized = COORDINATE_FIELDS.sub("", normalized)
    normalized = LOGGED_API_KEY.sub("--api-key=<REDACTED> origin=cli", normalized)
    normalized = unicodedata.normalize("NFC", normalized)
    return [line.rstrip() for line in normalized.replace("\r", "\n").splitlines() if line.strip()]


def _captured_logs(process_home: Path, fixture_root: Path, workspace_root: Path, server_url: str) -> list[dict[str, Any]]:
    log_root = process_home / ".cache" / "immich-go"
    if not log_root.exists():
        return []
    captured = []
    total_bytes = 0
    for path in sorted(log_root.glob("*.log")):
        data = path.read_bytes()
        total_bytes += len(data)
        if total_bytes > 4 * 1024 * 1024:
            raise OracleError("oracle logs exceeded the synthetic capture limit")
        text = data.decode("utf-8", errors="replace")
        normalized_lines = _normalize_text(text, fixture_root, workspace_root, server_url)
        semantic_markers = (
            " discovered ",
            " uploaded successfully ",
            " discarded local duplicate ",
            " metadata updated ",
            " stacked ",
            "JSON file detected",
            "Total Assets:",
            "  Processed:",
            "  Discarded:",
            "  Errors:",
            "  Pending:",
        )
        captured.append(
            {
                "name": LOG_TIMESTAMP.sub("<TIMESTAMP>", path.name),
                "lines": sorted(
                    line for line in normalized_lines if any(marker in line for marker in semantic_markers)
                ),
            }
        )
    return captured


def run_case(
    case_path: Path,
    executable: Path,
    baseline_path: Path = BASELINE_PATH,
    process_executor: Any = None,
) -> dict[str, Any]:
    """Verify, materialize, execute and normalize one black-box observation."""
    executable = executable.resolve()
    baseline = load_baseline(baseline_path)
    verified = verify_oracle(executable, baseline)
    case = load_case(case_path)
    materializer = _load_python(REPOSITORY_ROOT / "scripts" / "materialize-fixture.py", "oracle_fixture_materializer")
    mock_module = _load_python(REPOSITORY_ROOT / "tests" / "oracle" / "mock_immich_server.py", "oracle_mock_immich")
    capture_module = _load_python(REPOSITORY_ROOT / "scripts" / "bounded-process.py", "oracle_bounded_process")
    fixture_path = case["fixture_path"]
    fixture_manifest = materializer.load_manifest(fixture_path)
    fixture_digest = _sha256_file(fixture_path)

    with tempfile.TemporaryDirectory(prefix="immich-rs-oracle-") as temporary:
        temporary_root = Path(temporary)
        source_root = temporary_root / "fixture"
        process_home = temporary_root / "home"
        process_home.mkdir()
        materializer.materialize(fixture_path, source_root)
        with mock_module.running_mock() as server:
            substitutions = {
                "{server_url}": server.url,
                "{fixture_root}": str(source_root),
                "{synthetic_api_key}": mock_module.SYNTHETIC_API_KEY,
            }
            arguments = [substitutions.get(argument, argument) for argument in case["arguments"]]
            environment = {
                "HOME": str(process_home),
                "LANG": "C.UTF-8",
                "LC_ALL": "C.UTF-8",
                "NO_COLOR": "1",
                "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                "TZ": "UTC",
            }
            try:
                command = [str(executable), *arguments]
                if process_executor is None:
                    result = capture_module.run_command(
                        command, temporary_root, environment, case["timeout_seconds"]
                    )
                else:
                    result = process_executor(
                        command,
                        temporary_root,
                        environment,
                        case["timeout_seconds"],
                    )
            except (OSError, subprocess.TimeoutExpired) as error:
                raise OracleError(f"oracle execution failed safely: {error}") from error
            mock_observation = server.state.snapshot()

        requests = sorted(
            mock_observation["requests"],
            key=lambda request: (
                request["method"],
                request["path"],
                request["query"],
                request["body_sha256"],
            ),
        )
        committed = sorted(
            mock_observation["committed_mutations"],
            key=lambda request: (request["method"], request["path"], request["body_sha256"]),
        )
        grouped_requests: dict[str, dict[str, Any]] = {}
        for request in requests:
            key = json.dumps(
                {
                    "method": request["method"],
                    "path": request["path"],
                    "query": request["query"],
                    "body_sha256": request["body_sha256"],
                    "json_body": request["json_body"],
                    "mutating": request["mutating"],
                },
                sort_keys=True,
            )
            if key not in grouped_requests:
                grouped_requests[key] = {**json.loads(key), "count": 0}
            grouped_requests[key]["count"] += 1
        normalized_commits = [
            {
                "method": request["method"],
                "path": request["path"],
                "json_body": request["json_body"],
            }
            for request in committed
        ]
        observation = {
            "schema": ORACLE_OBSERVATION_SCHEMA,
            "case_id": case.get("case_id"),
            "oracle": verified,
            "fixture": {
                "schema": fixture_manifest["schema"],
                "fixture_id": fixture_manifest["fixture_id"],
                "manifest_sha256": fixture_digest,
            },
            "process": {
                "exit_code": result.returncode,
                "stdout": _normalize_text(result.stdout, source_root, temporary_root, server.url),
                "stderr": _normalize_text(result.stderr, source_root, temporary_root, server.url),
                "logs": _captured_logs(process_home, source_root, temporary_root, server.url),
            },
            "observable": {
                "all_requests_authenticated": all(request["authenticated"] for request in requests),
                "requests": [grouped_requests[key] for key in sorted(grouped_requests)],
                "committed_mutations": normalized_commits,
                "mutation_request_count": sum(1 for request in requests if request["mutating"]),
            },
        }
        actual_mutations = [
            {
                "method": request["method"],
                "path": request["path"],
                "json_body": request["json_body"],
                "classification": "oracle_defect",
            }
            for request in requests
            if request["mutating"]
        ]
        expected_mutations = sorted(
            case.get("expected_oracle_mutations", []),
            key=lambda item: (item["method"], item["path"]),
        )
        actual_mutations.sort(key=lambda item: (item["method"], item["path"]))
        if actual_mutations != expected_mutations:
            raise OracleError(
                "oracle mutation set differs from the explicit expectation: "
                + json.dumps(actual_mutations, sort_keys=True)
            )
        return observation


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", type=Path)
    parser.add_argument("--oracle", type=Path, default=Path(os.environ.get("IMMICH_GO_ORACLE", "/usr/local/bin/immich-go")))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--expect", type=Path)
    arguments = parser.parse_args()
    try:
        observation = run_case(arguments.case, arguments.oracle)
    except (OracleError, OSError, ValueError) as error:
        print(f"oracle runner failed: {error}", file=sys.stderr)
        return 2
    encoded = json.dumps(observation, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if arguments.expect:
        try:
            expected = arguments.expect.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            print(f"oracle runner failed: cannot read expected observation: {error}", file=sys.stderr)
            return 2
        if expected != encoded:
            difference = difflib.unified_diff(
                expected.splitlines(),
                encoded.splitlines(),
                fromfile=str(arguments.expect),
                tofile="actual-oracle-observation",
                lineterm="",
            )
            print("\n".join(difference), file=sys.stderr)
            return 1
        print(f"oracle observation matched {arguments.expect}")
        return 0
    if arguments.output:
        arguments.output.write_text(encoded, encoding="utf-8")
    else:
        sys.stdout.write(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
