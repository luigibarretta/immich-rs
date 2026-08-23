#!/usr/bin/env python3
"""Run paired end-to-end Google Takeout imports on one disposable Immich."""

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
import ssl
import statistics
import subprocess
import sys
import tomllib
from types import ModuleType
from typing import Any
from urllib import error as urlerror
from urllib import request

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "tests/oracle/baseline.toml"
METRICS = (
    "wall_time_seconds", "user_cpu_seconds", "system_cpu_seconds", "peak_rss_bytes",
    "peak_open_file_descriptors", "characters_read", "characters_written",
    "storage_bytes_read", "storage_bytes_written",
)
SUM_METRICS = set(METRICS) - {"peak_rss_bytes", "peak_open_file_descriptors"}
MAX_RESPONSE_BYTES = 1_048_576


class BenchmarkError(RuntimeError):
    """A paired Takeout benchmark invariant failed."""


def load_module(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise BenchmarkError(f"cannot load {path.name}")
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


def corpus_sha256(source: Path) -> str:
    digest = hashlib.sha256()
    files = sorted(path for path in source.rglob("*") if path.is_file())
    for path in files:
        digest.update(path.relative_to(source).as_posix().encode())
        digest.update(b"\0")
        with path.open("rb") as handle:
            while chunk := handle.read(1_048_576):
                digest.update(chunk)
    return digest.hexdigest()


def api(endpoint: str, path: str, ca: Path, method="GET", body=None, token=None) -> Any:
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body, separators=(",", ":")).encode()
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    query = request.Request(endpoint + path, data=payload, headers=headers, method=method)
    try:
        context = ssl.create_default_context(cafile=str(ca))
        opener = request.build_opener(request.ProxyHandler({}), request.HTTPSHandler(context=context))
        with opener.open(query, timeout=30) as response:
            encoded = response.read(MAX_RESPONSE_BYTES + 1)
    except urlerror.HTTPError as error:
        raise BenchmarkError(f"disposable API request failed for {path}: HTTP {error.code}") from error
    except (OSError, urlerror.URLError) as error:
        reason = getattr(error, "reason", error)
        raise BenchmarkError(
            f"disposable API request failed for {path}: {type(reason).__name__}"
        ) from error
    if len(encoded) > MAX_RESPONSE_BYTES:
        raise BenchmarkError("disposable API response exceeded its bound")
    try:
        return json.loads(encoded)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise BenchmarkError("disposable API returned invalid JSON") from error


def create_account(endpoint: str, ca: Path, admin: str, run_id: str, index: int, tool: str) -> tuple[str, str]:
    email = f"synthetic-{run_id}-{index}-{tool}@example.invalid"
    password = "Synthetic-only-" + secrets.token_urlsafe(24)
    api(endpoint, "/api/admin/users", ca, "POST", {
        "email": email, "name": "Synthetic Phase8 Benchmark",
        "password": password, "shouldChangePassword": False,
    }, admin)
    login = api(endpoint, "/api/auth/login", ca, "POST", {"email": email, "password": password})
    token = login.get("accessToken") if isinstance(login, dict) else None
    if not isinstance(token, str) or not token:
        raise BenchmarkError("disposable login omitted its access token")
    key = api(endpoint, "/api/api-keys", ca, "POST", {
        "name": "immich-rs paired Takeout benchmark",
        "permissions": ["asset.read", "asset.statistics", "asset.upload", "asset.update", "server.about", "user.read"],
    }, token)
    secret = key.get("secret") if isinstance(key, dict) else None
    if not isinstance(secret, str) or not secret:
        raise BenchmarkError("disposable API key creation failed")
    return token, secret


def load_fixture(path: Path, source: Path) -> dict[str, Any]:
    fixture = json.loads(path.read_text(encoding="utf-8"))
    expected = {"media_assets": 8, "metadata_updates": 8, "media_bytes": 67_108_864, "concurrency": 1}
    provenance = fixture.get("provenance") if isinstance(fixture, dict) else None
    if fixture.get("schema") != "fixture-manifest-v1" or fixture.get("benchmark") != expected:
        raise BenchmarkError("benchmark fixture contract drifted")
    if provenance != {"kind": "synthetic", "generator": "scripts/materialize-fixture.py", "generator_version": "1", "license": "CC0-1.0"}:
        raise BenchmarkError("benchmark fixture provenance drifted")
    files = fixture.get("files")
    if not isinstance(files, list) or len(files) != 16:
        raise BenchmarkError("benchmark fixture file set drifted")
    media = [source / item["path"] for item in files if item.get("recipe") == "synthetic_padded_png"]
    if len(media) != 8 or any(not path.is_file() or path.stat().st_size != 8_388_608 for path in media):
        raise BenchmarkError("materialized benchmark corpus drifted")
    return fixture


def environment(workspace: Path, ca: Path, api_key: str | None = None) -> dict[str, str]:
    home = workspace / "home"
    home.mkdir(exist_ok=True)
    selected = {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC",
        "SSL_CERT_FILE": str(ca),
    }
    if api_key is not None:
        selected["IMMICH_RS_API_KEY"] = api_key
    return selected


def combine(values: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        field: (sum(item[field] for item in values) if field in SUM_METRICS else max(item[field] for item in values))
        for field in METRICS
    }


def outcome(endpoint: str, ca: Path, token: str) -> dict[str, int]:
    statistics_value = api(endpoint, "/api/assets/statistics", ca, token=token)
    search = api(endpoint, "/api/search/metadata", ca, "POST", {"size": 100, "withExif": True}, token)
    items = search.get("assets", {}).get("items") if isinstance(search, dict) else None
    descriptions = sum(
        isinstance(item, dict) and isinstance(item.get("exifInfo"), dict)
        and bool(item["exifInfo"].get("description")) for item in items or []
    )
    if not isinstance(items, list) or statistics_value.get("total") != 8 or len(items) != 8 or descriptions != 8:
        raise BenchmarkError("benchmark server outcome drifted")
    return {"source_operations": 8, "metadata_assignments": descriptions, "visible_assets": 8, "retries": 0}


def run_rust(binary: Path, endpoint: str, ca: Path, source: Path, workspace: Path, key: str, metrics: ModuleType) -> dict[str, Any]:
    plan_path = workspace / "plan.json"
    checkpoint = workspace / "checkpoint.sqlite"
    env = environment(workspace, ca, key)
    plan_command = [
        str(binary), "plan", "upload", "google-takeout", "--server", endpoint,
        "--authorize-production-read", "--ca-certificate", str(ca),
        "--label", "synthetic-phase8-benchmark", str(source),
    ]
    planned, plan_metrics = metrics.run_command(plan_command, workspace, env, 300)
    if planned.returncode != 0:
        raise BenchmarkError("immich-rs Takeout planning failed")
    try:
        plan = json.loads(planned.stdout)
    except json.JSONDecodeError as error:
        raise BenchmarkError("immich-rs emitted an invalid plan") from error
    summary = plan.get("summary")
    if not isinstance(summary, dict) or summary.get("operations") != 8 or summary.get("metadata_updates") != 8 or summary.get("max_mutations") != 16:
        raise BenchmarkError("immich-rs Takeout plan counters drifted")
    plan_path.write_text(planned.stdout, encoding="utf-8")
    plan_path.chmod(0o600)
    compact = json.dumps(plan, ensure_ascii=False, separators=(",", ":")).encode()
    plan_digest = hashlib.sha256(compact).hexdigest()
    apply_command = [
        str(binary), "apply", "upload", "--server", endpoint, "--ca-certificate", str(ca),
        "--authorize-production-read", "--authorize-production-write",
        "--confirm-plan-sha256", plan_digest, "--expected-operations", "16",
        "--backup-reference", "synthetic-phase8-benchmark-backup", "--plan", str(plan_path),
        "--source", str(source), "--checkpoint", str(checkpoint),
    ]
    applied, apply_metrics = metrics.run_command(apply_command, workspace, env, 300)
    if applied.returncode != 0:
        raise BenchmarkError("immich-rs Takeout apply failed")
    report = json.loads(applied.stdout)
    if report.get("created") != 8 or report.get("metadata_updated") != 8 or report.get("retried") != 0:
        raise BenchmarkError("immich-rs Takeout apply counters drifted")
    return combine([plan_metrics, apply_metrics])


def run_go(binary: Path, endpoint: str, ca: Path, source: Path, workspace: Path, key: str, metrics: ModuleType) -> dict[str, Any]:
    command = [
        str(binary), "upload", "from-google-photos", "--server", endpoint, "--api-key", key,
        "--device-uuid", "synthetic-phase8-benchmark", "--no-ui", "--log-level", "INFO",
        "--pause-immich-jobs=false", "--concurrent-tasks", "1", "--sync-albums=false",
        "--takeout-tag=false", "--people-tag=false", str(source),
    ]
    completed, measured = metrics.run_command(command, workspace, environment(workspace, ca), 300)
    if completed.returncode != 0:
        details = completed.stdout + "\n" + completed.stderr
        for sensitive in (key, endpoint, str(ca), str(source), str(workspace)):
            details = details.replace(sensitive, "<REDACTED>")
        raise BenchmarkError(f"immich-go Takeout import failed: {details[-1000:]}")
    if "retry" in (completed.stdout + completed.stderr).casefold():
        raise BenchmarkError("immich-go reported a retry")
    return measured


def aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result = {}
    for field in METRICS:
        values = sorted(sample[tool][field] for sample in samples)
        result[field] = {"min": min(values), "median": statistics.median(values), "p95": values[-1], "max": max(values)}
    return result


def tool_version(command: list[str]) -> str:
    completed = subprocess.run(
        command,
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
        env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8"},
    )
    return (completed.stdout or completed.stderr).strip()


def run(options: argparse.Namespace) -> dict[str, Any]:
    if not re.fullmatch(r"https://[^/]+", options.endpoint) or not re.fullmatch(r"[0-9a-f]{40}", options.source_revision):
        raise BenchmarkError("benchmark endpoint or revision is invalid")
    if not 2 <= options.samples <= 10 or not 0 <= options.warmups <= 2:
        raise BenchmarkError("samples must be 2..10 and warmups 0..2")
    admin = os.environ.get("IMMICH_RS_BENCHMARK_ADMIN_TOKEN")
    if not admin:
        raise BenchmarkError("disposable admin token is missing")
    metrics = load_module(ROOT / "scripts/benchmark-metrics.py", "phase8_metrics")
    fixture = load_fixture(options.fixture_manifest, options.source)
    raw = []
    for index in range(options.warmups + options.samples):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        pair: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            workspace = options.workspace / f"pair-{index}-{tool}"
            workspace.mkdir(parents=True)
            token, key = create_account(options.endpoint, options.ca_certificate, admin, options.run_id, index, tool)
            measured = run_rust(options.immich_rs, options.endpoint, options.ca_certificate, options.source, workspace, key, metrics) if tool == "immich_rs" else run_go(options.oracle, options.endpoint, options.ca_certificate, options.source, workspace, key, metrics)
            measured["operations"] = outcome(options.endpoint, options.ca_certificate, token)
            pair[tool] = measured
        if index >= options.warmups:
            pair["sample"] = index - options.warmups
            raw.append(pair)
    aggregates = {tool: aggregate(raw, tool) for tool in ("immich_rs", "immich_go")}
    rust_wall, go_wall = aggregates["immich_rs"]["wall_time_seconds"], aggregates["immich_go"]["wall_time_seconds"]
    improvement = 100 * (go_wall["median"] - rust_wall["median"]) / go_wall["median"]
    claim = "Raw measurements only; no performance improvement is claimed."
    if improvement >= 10 and rust_wall["max"] < go_wall["min"]:
        claim = f"On this exact 67,108,864-byte eight-asset synthetic Takeout import, immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; this is not a large-library, WAN or production claim."
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    return {
        "schema": "phase8-benchmark-report-v1",
        "manifest": {
            "source_revision": options.source_revision,
            "fixture": {"id": fixture["fixture_id"], "kind": "synthetic", "license": "CC0-1.0", "manifest_sha256": sha256(options.fixture_manifest), "corpus_sha256": corpus_sha256(options.source), **fixture["benchmark"]},
            "tools": {"immich_rs": {"version": tool_version([str(options.immich_rs), "--version"]), "sha256": sha256(options.immich_rs)}, "immich_go": {**baseline["oracle"], **baseline["artifacts"]["linux_x86_64"], "sha256": sha256(options.oracle)}},
            "environment": {"system": platform.system(), "kernel": platform.release(), "architecture": platform.machine(), "logical_cpus": os.cpu_count(), "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>"},
            "methodology": {"samples": options.samples, "warmups": options.warmups, "pairing": "fresh isolated owner per tool on one disposable HTTPS server and one corpus", "order": "alternating within pairs", "scope": "complete plan plus import; account setup, server startup and postcondition probes excluded", "concurrency": 1, "percentiles": "nearest-rank p95 over the raw samples"},
        },
        "raw_samples": raw, "aggregate": aggregates, "claims": [claim],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--ca-certificate", type=Path, required=True)
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
    options = parser.parse_args()
    try:
        report = run(options)
        options.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (BenchmarkError, OSError, UnicodeError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Phase 8 benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
