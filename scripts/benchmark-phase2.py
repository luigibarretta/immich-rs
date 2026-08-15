#!/usr/bin/env python3
"""Run paired Phase-2 uploads against fresh users on one disposable Immich."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import secrets
import statistics
import subprocess
import sys
import tomllib
from types import ModuleType
from typing import Any
from urllib import error as urlerror
from urllib import request


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
BASELINE_PATH = REPOSITORY_ROOT / "tests" / "oracle" / "baseline.toml"
MAX_RESPONSE_BYTES = 1_048_576
METRICS = (
    "wall_time_seconds",
    "user_cpu_seconds",
    "system_cpu_seconds",
    "peak_rss_bytes",
    "peak_open_file_descriptors",
    "characters_read",
    "characters_written",
    "storage_bytes_read",
    "storage_bytes_written",
)
SUM_METRICS = set(METRICS) - {"peak_rss_bytes", "peak_open_file_descriptors"}


class Phase2BenchmarkError(RuntimeError):
    """A Phase-2 benchmark invariant failed."""


def load_python(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise Phase2BenchmarkError(f"cannot load {path.name}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def command_output(command: list[str]) -> str:
    try:
        completed = subprocess.run(command, check=True, capture_output=True, text=True, timeout=15)
    except (OSError, subprocess.SubprocessError) as failure:
        raise Phase2BenchmarkError("cannot inspect a benchmark tool") from failure
    return (completed.stdout or completed.stderr).strip()


def api(endpoint: str, path: str, method: str = "GET", body: object | None = None, token: str | None = None) -> dict[str, Any]:
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body, separators=(",", ":")).encode()
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    api_request = request.Request(endpoint + path, data=payload, headers=headers, method=method)
    try:
        with request.urlopen(api_request, timeout=20) as response:
            encoded = response.read(MAX_RESPONSE_BYTES + 1)
    except (OSError, urlerror.URLError) as failure:
        raise Phase2BenchmarkError(f"disposable API request failed for {path}") from failure
    if len(encoded) > MAX_RESPONSE_BYTES:
        raise Phase2BenchmarkError("disposable API response exceeded its bound")
    try:
        value = json.loads(encoded)
    except (UnicodeError, json.JSONDecodeError) as failure:
        raise Phase2BenchmarkError("disposable API returned invalid JSON") from failure
    if not isinstance(value, dict):
        raise Phase2BenchmarkError("disposable API response was not an object")
    return value


def create_account(endpoint: str, admin_token: str, run_id: str, index: int, tool: str) -> tuple[str, str]:
    email = f"synthetic-{run_id}-{index}-{tool}@example.invalid"
    password = "Synthetic-only-" + secrets.token_urlsafe(24)
    api(
        endpoint,
        "/api/admin/users",
        "POST",
        {"email": email, "name": "Synthetic Phase2 Benchmark", "password": password, "shouldChangePassword": False},
        admin_token,
    )
    login = api(endpoint, "/api/auth/login", "POST", {"email": email, "password": password})
    access_token = login.get("accessToken")
    if not isinstance(access_token, str) or not access_token:
        raise Phase2BenchmarkError("disposable login omitted its access token")
    key = api(
        endpoint,
        "/api/api-keys",
        "POST",
        {"name": "immich-rs paired benchmark", "permissions": ["asset.read", "asset.statistics", "asset.upload", "server.about", "user.read"]},
        access_token,
    ).get("secret")
    if not isinstance(key, str) or not key:
        raise Phase2BenchmarkError("disposable API key creation failed")
    return access_token, key


def load_fixture(path: Path, source: Path) -> dict[str, Any]:
    fixture = json.loads(path.read_text(encoding="utf-8"))
    expected = {
        "upload_operations": 4,
        "xmp_sidecars": 1,
        "live_photo_pairs": 0,
        "visible_assets": 4,
        "live_photo_links": 0,
    }
    if fixture.get("schema") != "phase2-benchmark-corpus-v1" or fixture.get("expected") != expected:
        raise Phase2BenchmarkError("benchmark fixture contract drifted")
    files = fixture.get("files")
    if not isinstance(files, list) or len(files) != 5:
        raise Phase2BenchmarkError("benchmark fixture file matrix drifted")
    for entry_value in files:
        if not isinstance(entry_value, dict) or not isinstance(entry_value.get("path"), str):
            raise Phase2BenchmarkError("benchmark fixture entry is invalid")
        media_path = source / entry_value["path"]
        if not media_path.is_file() or media_path.stat().st_size != entry_value.get("bytes"):
            raise Phase2BenchmarkError("benchmark fixture length drifted")
        if sha256(media_path) != entry_value.get("sha256"):
            raise Phase2BenchmarkError("benchmark fixture digest drifted")
    return fixture


def outcome(endpoint: str, access_token: str, expected: dict[str, int]) -> dict[str, int]:
    statistics = api(endpoint, "/api/assets/statistics", token=access_token)
    search = api(endpoint, "/api/search/metadata", "POST", {"size": 100}, access_token)
    items = search.get("assets", {}).get("items") if isinstance(search.get("assets"), dict) else None
    if not isinstance(items, list):
        raise Phase2BenchmarkError("disposable search response omitted assets")
    visible = statistics.get("total")
    links = sum(isinstance(item, dict) and item.get("livePhotoVideoId") is not None for item in items)
    if visible != expected["visible_assets"] or links != expected["live_photo_links"]:
        raise Phase2BenchmarkError(
            f"benchmark upload matrix drifted: visible_assets={visible}, live_photo_links={links}"
        )
    return {
        "source_operations": expected["upload_operations"],
        "visible_assets": visible,
        "live_photo_links": links,
        "retries": 0,
    }


def combine(measurements: list[dict[str, Any]]) -> dict[str, Any]:
    combined: dict[str, Any] = {}
    for field in METRICS:
        values = [measurement[field] for measurement in measurements]
        combined[field] = sum(values) if field in SUM_METRICS else max(values)
    return combined


def redacted_failure(completed: subprocess.CompletedProcess[str], secrets_to_hide: list[str]) -> str:
    details = (completed.stdout + "\n" + completed.stderr).strip()
    for sensitive in secrets_to_hide:
        if sensitive:
            details = details.replace(sensitive, "<REDACTED>")
    return details[-2_000:] if details else "no process diagnostics"


def safe_environment(workspace: Path, api_key: str | None = None) -> dict[str, str]:
    home = workspace / "home"
    home.mkdir(exist_ok=True)
    environment = {
        "HOME": str(home),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "TZ": "UTC",
    }
    if api_key is not None:
        environment["IMMICH_RS_API_KEY"] = api_key
    return environment


def run_immich_rs(
    binary: Path,
    endpoint: str,
    source: Path,
    workspace: Path,
    api_key: str,
    metrics: ModuleType,
    expected: dict[str, int],
) -> dict[str, Any]:
    plan_path = workspace / "upload-plan.json"
    checkpoint = workspace / "checkpoint.sqlite"
    environment = safe_environment(workspace, api_key)
    plan_command = [
        str(binary), "plan", "upload", "folder", "--server", endpoint,
        "--label", "synthetic-phase2-benchmark", "--buffer-bytes", "65536", str(source),
    ]
    try:
        planned, plan_metrics = metrics.run_command(plan_command, workspace, environment, 180)
    except (OSError, RuntimeError, subprocess.SubprocessError) as failure:
        raise Phase2BenchmarkError("immich-rs planning process failed") from failure
    if planned.returncode != 0:
        raise Phase2BenchmarkError("immich-rs planning failed")
    try:
        plan = json.loads(planned.stdout)
    except json.JSONDecodeError as failure:
        raise Phase2BenchmarkError("immich-rs emitted an invalid upload plan") from failure
    summary = plan.get("summary")
    if not isinstance(summary, dict) or any(
        summary.get(field) != expected[key]
        for field, key in (
            ("operations", "upload_operations"),
            ("xmp_sidecars", "xmp_sidecars"),
            ("live_photo_pairs", "live_photo_pairs"),
        )
    ):
        raise Phase2BenchmarkError("immich-rs upload plan matrix drifted")
    plan_path.write_text(planned.stdout, encoding="utf-8")
    plan_path.chmod(0o600)
    apply_command = [
        str(binary), "apply", "upload", "--server", endpoint, "--plan", str(plan_path),
        "--source", str(source), "--checkpoint", str(checkpoint), "--buffer-bytes", "65536",
    ]
    try:
        applied, apply_metrics = metrics.run_command(apply_command, workspace, environment, 180)
    except (OSError, RuntimeError, subprocess.SubprocessError) as failure:
        raise Phase2BenchmarkError("immich-rs apply process failed") from failure
    if applied.returncode != 0:
        raise Phase2BenchmarkError("immich-rs apply failed")
    try:
        report = json.loads(applied.stdout)
    except json.JSONDecodeError as failure:
        raise Phase2BenchmarkError("immich-rs emitted an invalid apply report") from failure
    if report.get("created") != expected["upload_operations"] or report.get("retried") != 0:
        raise Phase2BenchmarkError("immich-rs apply counters drifted")
    return combine([plan_metrics, apply_metrics])


def run_immich_go(
    binary: Path, endpoint: str, source: Path, workspace: Path, api_key: str, metrics: ModuleType
) -> dict[str, Any]:
    command = [
        str(binary), "upload", "from-folder", "--server", endpoint, "--api-key", api_key, "--no-ui",
        "--concurrent-tasks", "1", "--pause-immich-jobs=false", "--device-uuid",
        "synthetic-phase2-benchmark", str(source),
    ]
    try:
        completed, measured = metrics.run_command(command, workspace, safe_environment(workspace), 180)
    except (OSError, RuntimeError, subprocess.SubprocessError) as failure:
        raise Phase2BenchmarkError("immich-go upload process failed") from failure
    if completed.returncode != 0:
        details = redacted_failure(completed, [api_key, endpoint, str(source), str(workspace)])
        raise Phase2BenchmarkError(f"immich-go upload failed: {details}")
    if "retry" in (completed.stdout + completed.stderr).casefold():
        raise Phase2BenchmarkError("immich-go reported a retry in the no-fault benchmark")
    return measured


def aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for field in METRICS:
        values = sorted(sample[tool][field] for sample in samples)
        p95_index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {"min": min(values), "median": statistics.median(values), "p95": values[p95_index], "max": max(values)}
    return result


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    if not re.fullmatch(r"http://127\.0\.0\.1:[0-9]{1,5}", arguments.endpoint):
        raise Phase2BenchmarkError("benchmark endpoint must be IPv4 loopback")
    if not re.fullmatch(r"[0-9a-f]{40}", arguments.source_revision):
        raise Phase2BenchmarkError("source revision must be an exact commit")
    if not re.fullmatch(r"[a-zA-Z0-9.-]{8,80}", arguments.run_id):
        raise Phase2BenchmarkError("run ID is invalid")
    if arguments.samples < 2 or arguments.samples > 10 or arguments.warmups < 0 or arguments.warmups > 2:
        raise Phase2BenchmarkError("samples must be 2..10 and warmups must be 0..2")
    admin_token = os.environ.get("IMMICH_RS_BENCHMARK_ADMIN_TOKEN")
    if not admin_token:
        raise Phase2BenchmarkError("disposable admin token is missing")
    metrics = load_python(REPOSITORY_ROOT / "scripts" / "benchmark-metrics.py", "phase2_metrics")
    fixture = load_fixture(arguments.fixture_manifest, arguments.source)
    expected = fixture["expected"]
    raw: list[dict[str, Any]] = []
    for index in range(arguments.warmups + arguments.samples):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        sample: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            tool_workspace = arguments.workspace / f"sample-{index}-{tool}"
            tool_workspace.mkdir(parents=True)
            access_token, api_key = create_account(arguments.endpoint, admin_token, arguments.run_id, index, tool)
            if tool == "immich_rs":
                measured = run_immich_rs(
                    arguments.immich_rs,
                    arguments.endpoint,
                    arguments.source,
                    tool_workspace,
                    api_key,
                    metrics,
                    expected,
                )
            else:
                measured = run_immich_go(arguments.oracle, arguments.endpoint, arguments.source, tool_workspace, api_key, metrics)
            measured["operations"] = outcome(arguments.endpoint, access_token, expected)
            sample[tool] = measured
        if index >= arguments.warmups:
            sample["sample"] = index - arguments.warmups
            raw.append(sample)
    baseline = tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    return {
        "schema": "phase2-benchmark-report-v1",
        "manifest": {
            "source_revision": arguments.source_revision,
            "fixture": {"manifest_sha256": sha256(arguments.fixture_manifest), "derived_from": fixture["derived_from"], "files": fixture["files"], "expected": expected},
            "tools": {
                "immich_rs": {"version": command_output([str(arguments.immich_rs), "--version"]), "sha256": sha256(arguments.immich_rs)},
                "immich_go": {**baseline["oracle"], **baseline["artifacts"]["linux_x86_64"], "sha256": sha256(arguments.oracle)},
            },
            "environment": {"system": platform.system(), "kernel": platform.release(), "architecture": platform.machine(), "logical_cpus": os.cpu_count(), "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>"},
            "methodology": {"samples": arguments.samples, "warmups": arguments.warmups, "pairing": "fresh isolated owner per tool on one disposable server and one corpus", "order": "alternating within pairs", "scope": "client processes only; account setup, server startup and postcondition probes excluded", "immich_rs_workflow": "plan upload folder plus apply upload", "concurrency": "immich-rs default 1; immich-go concurrent-tasks 1", "percentiles": "nearest-rank p95 over the raw samples"},
        },
        "raw_samples": raw,
        "aggregate": {"immich_rs": aggregate(raw, "immich_rs"), "immich_go": aggregate(raw, "immich_go")},
        "claims": ["Raw measurements only; no performance improvement is claimed."],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--fixture-manifest", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--immich-rs", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        report = run(arguments)
        arguments.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, UnicodeError, ValueError, KeyError, Phase2BenchmarkError) as failure:
        print(f"Phase 2 benchmark failed: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
