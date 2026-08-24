#!/usr/bin/env python3
"""Benchmark paired migrations between two disposable Immich servers."""

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
    """A real-server migration benchmark invariant failed."""


def load_metrics() -> ModuleType:
    path = ROOT / "scripts/benchmark-metrics.py"
    spec = importlib.util.spec_from_file_location("real_migration_metrics", path)
    if spec is None or spec.loader is None:
        raise BenchmarkError("cannot load process metrics")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
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
    for path in sorted(item for item in source.rglob("*") if item.is_file()):
        digest.update(path.relative_to(source).as_posix().encode())
        digest.update(b"\0")
        with path.open("rb") as handle:
            while chunk := handle.read(1_048_576):
                digest.update(chunk)
    return digest.hexdigest()


def api(endpoint: str, path: str, method="GET", body=None, token=None) -> Any:
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body, separators=(",", ":")).encode()
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    query = request.Request(endpoint + path, data=payload, headers=headers, method=method)
    try:
        with request.build_opener(request.ProxyHandler({})).open(query, timeout=30) as response:
            encoded = response.read(MAX_RESPONSE_BYTES + 1)
    except urlerror.HTTPError as error:
        raise BenchmarkError(f"disposable API request failed for {path}: HTTP {error.code}") from error
    except (OSError, urlerror.URLError) as error:
        reason = getattr(error, "reason", error)
        raise BenchmarkError(f"disposable API request failed: {type(reason).__name__}") from error
    if len(encoded) > MAX_RESPONSE_BYTES:
        raise BenchmarkError("disposable API response exceeded its bound")
    try:
        return json.loads(encoded)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise BenchmarkError("disposable API returned invalid JSON") from error


def create_account(endpoint: str, admin: str, run_id: str, index: int, role: str) -> tuple[str, str]:
    email = f"synthetic-{run_id}-{index}-{role}@example.invalid"
    password = "Synthetic-only-" + secrets.token_urlsafe(24)
    api(endpoint, "/api/admin/users", "POST", {
        "email": email, "name": "Synthetic Migration Benchmark",
        "password": password, "shouldChangePassword": False,
    }, admin)
    login = api(endpoint, "/api/auth/login", "POST", {"email": email, "password": password})
    token = login.get("accessToken") if isinstance(login, dict) else None
    if not isinstance(token, str) or not token:
        raise BenchmarkError("disposable login omitted its access token")
    permissions = [
        "asset.download", "asset.read", "asset.statistics", "asset.update", "asset.upload",
        "album.read", "album.create", "albumAsset.create", "server.about", "user.read",
    ]
    key = api(endpoint, "/api/api-keys", "POST", {
        "name": "immich-rs paired migration benchmark", "permissions": permissions,
    }, token)
    secret = key.get("secret") if isinstance(key, dict) else None
    if not isinstance(secret, str) or not secret:
        raise BenchmarkError("disposable API key creation failed")
    return token, secret


def safe_environment(workspace: Path, **values: str) -> dict[str, str]:
    home = workspace / "home"
    home.mkdir(exist_ok=True)
    return {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "NO_COLOR": "1",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC", **values,
    }


def seed_source(options: argparse.Namespace, workspace: Path, key: str) -> None:
    plan = workspace / "seed-plan.json"
    checkpoint = workspace / "seed.sqlite"
    environment = safe_environment(workspace, IMMICH_RS_API_KEY=key)
    planned = subprocess.run([
        str(options.immich_rs), "plan", "upload", "google-takeout",
        "--server", options.source_endpoint, str(options.source),
    ], cwd=workspace, env=environment, capture_output=True, text=True, timeout=300, check=False)
    if planned.returncode != 0:
        raise BenchmarkError("source seed planning failed")
    plan.write_text(planned.stdout, encoding="utf-8")
    plan.chmod(0o600)
    applied = subprocess.run([
        str(options.immich_rs), "apply", "upload", "--server", options.source_endpoint,
        "--plan", str(plan), "--source", str(options.source), "--checkpoint", str(checkpoint),
    ], cwd=workspace, env=environment, capture_output=True, text=True, timeout=300, check=False)
    if applied.returncode != 0 or json.loads(applied.stdout).get("created") != 8:
        raise BenchmarkError("source seed apply failed")


def resource_arguments() -> list[str]:
    return [
        "--page-size", "10", "--max-assets", "10", "--max-albums", "10",
        "--max-album-memberships", "20", "--max-asset-bytes", "16777216",
        "--max-total-bytes", "134217728", "--concurrency", "1",
    ]


def combine(values: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        field: sum(item[field] for item in values) if field in SUM_METRICS
        else max(item[field] for item in values)
        for field in METRICS
    }


def run_rust(
    options: argparse.Namespace, workspace: Path, source_key: str,
    destination_key: str, metrics: ModuleType,
) -> dict[str, Any]:
    plan = workspace / "migration-plan.json"
    checkpoint = workspace / "migration.sqlite"
    common = [
        "--source-server", options.source_endpoint,
        "--destination-server", options.destination_endpoint,
    ]
    environment = safe_environment(
        workspace, IMMICH_RS_SOURCE_API_KEY=source_key,
        IMMICH_RS_DESTINATION_API_KEY=destination_key,
    )
    planned, plan_metrics = metrics.run_command([
        str(options.immich_rs), "plan", "migration", "immich",
        *common, *resource_arguments(),
    ], workspace, environment, 300)
    if planned.returncode != 0:
        raise BenchmarkError("immich-rs migration planning failed")
    value = json.loads(planned.stdout)
    if value.get("summary") != {
        "assets": 8, "media_bytes": 67_108_864, "metadata_updates": 8,
        "album_creates": 0, "album_memberships": 0, "max_mutations": 16,
    }:
        raise BenchmarkError("immich-rs migration plan counters drifted")
    plan.write_text(planned.stdout, encoding="utf-8")
    plan.chmod(0o600)
    applied, apply_metrics = metrics.run_command([
        str(options.immich_rs), "apply", "migration", "immich", "--plan", str(plan),
        "--checkpoint", str(checkpoint), *common, *resource_arguments(),
    ], workspace, environment, 300)
    if applied.returncode != 0:
        raise BenchmarkError("immich-rs migration apply failed")
    report = json.loads(applied.stdout)
    if report.get("created") != 8 or report.get("metadata_updated") != 8 or report.get("retried") != 0:
        raise BenchmarkError("immich-rs migration report drifted")
    return combine([plan_metrics, apply_metrics])


def run_go(
    options: argparse.Namespace, workspace: Path, source_key: str,
    destination_key: str, metrics: ModuleType,
) -> dict[str, Any]:
    completed, measured = metrics.run_command([
        str(options.oracle), "upload", "from-immich", "--from-server", options.source_endpoint,
        "--from-api-key", source_key, "--server", options.destination_endpoint,
        "--api-key", destination_key, "--from-device-uuid", "synthetic-source-benchmark",
        "--device-uuid", "synthetic-destination-benchmark", "--from-pause-immich-jobs=false",
        "--pause-immich-jobs=false", "--no-ui", "--log-level", "ERROR",
        "--concurrent-tasks", "1", "--on-errors", "stop",
    ], workspace, safe_environment(workspace), 300)
    if completed.returncode != 0:
        details = completed.stdout + "\n" + completed.stderr
        for sensitive in (
            source_key, destination_key, options.source_endpoint,
            options.destination_endpoint, str(workspace),
        ):
            details = details.replace(sensitive, "<REDACTED>")
        raise BenchmarkError(f"immich-go migration failed: {details[-1000:]}")
    return measured


def outcome(options: argparse.Namespace, source_token: str, destination_token: str) -> dict[str, int]:
    source = api(options.source_endpoint, "/api/assets/statistics", token=source_token)
    destination = api(options.destination_endpoint, "/api/assets/statistics", token=destination_token)
    search = api(
        options.destination_endpoint, "/api/search/metadata", "POST",
        {"size": 100, "withExif": True}, destination_token,
    )
    items = search.get("assets", {}).get("items") if isinstance(search, dict) else None
    descriptions = sum(
        isinstance(item, dict) and isinstance(item.get("exifInfo"), dict)
        and bool(item["exifInfo"].get("description")) for item in items or []
    )
    if source.get("total") != 8 or destination.get("total") != 8 or not isinstance(items, list) or len(items) != 8 or descriptions != 8:
        raise BenchmarkError("migration server outcome drifted")
    return {
        "source_assets": 8, "destination_assets": 8, "metadata_updates": 8,
        "media_bytes": 67_108_864, "retries": 0,
    }


def aggregate(samples: list[dict[str, Any]], tool: str) -> dict[str, Any]:
    result = {}
    for field in METRICS:
        values = sorted(sample[tool][field] for sample in samples)
        index = max(0, (95 * len(values) + 99) // 100 - 1)
        result[field] = {
            "min": min(values), "median": statistics.median(values),
            "p95": values[index], "max": max(values),
        }
    return result


def version(binary: Path) -> str:
    completed = subprocess.run(
        [str(binary), "--version"], check=True, capture_output=True, text=True,
        timeout=15, env={"LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "")},
    )
    return (completed.stdout or completed.stderr).strip()


def run(options: argparse.Namespace) -> dict[str, Any]:
    endpoint = r"http://127\.0\.0\.1:[0-9]+"
    if re.fullmatch(endpoint, options.source_endpoint) is None or re.fullmatch(endpoint, options.destination_endpoint) is None or options.source_endpoint == options.destination_endpoint:
        raise BenchmarkError("benchmark requires two distinct loopback servers")
    if re.fullmatch(r"[0-9a-f]{40}", options.source_revision) is None:
        raise BenchmarkError("source revision must be an exact SHA")
    if not 2 <= options.samples <= 10 or not 0 <= options.warmups <= 2:
        raise BenchmarkError("sample or warmup count is outside its bound")
    source_admin = os.environ.get("IMMICH_RS_BENCHMARK_SOURCE_ADMIN_TOKEN")
    destination_admin = os.environ.get("IMMICH_RS_BENCHMARK_DESTINATION_ADMIN_TOKEN")
    if not source_admin or not destination_admin:
        raise BenchmarkError("disposable admin tokens are missing")
    metrics = load_metrics()
    raw = []
    for index in range(options.samples + options.warmups):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        pair: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            workspace = options.workspace / f"pair-{index}-{tool}"
            workspace.mkdir(parents=True)
            source_token, source_key = create_account(
                options.source_endpoint, source_admin, options.run_id, index, f"{tool}-source",
            )
            destination_token, destination_key = create_account(
                options.destination_endpoint, destination_admin, options.run_id, index,
                f"{tool}-destination",
            )
            seed_source(options, workspace, source_key)
            measured = run_rust(options, workspace, source_key, destination_key, metrics) if tool == "immich_rs" else run_go(options, workspace, source_key, destination_key, metrics)
            measured["operations"] = outcome(options, source_token, destination_token)
            pair[tool] = measured
        if index >= options.warmups:
            pair["sample"] = index - options.warmups
            raw.append(pair)
    aggregates = {tool: aggregate(raw, tool) for tool in ("immich_rs", "immich_go")}
    rust_wall = aggregates["immich_rs"]["wall_time_seconds"]
    go_wall = aggregates["immich_go"]["wall_time_seconds"]
    improvement = 100 * (go_wall["median"] - rust_wall["median"]) / go_wall["median"]
    claim = "Raw measurements only; no performance improvement is claimed."
    if improvement >= 10 and rust_wall["max"] < go_wall["min"]:
        claim = (
            "On this exact 67,108,864-byte eight-asset synthetic real-server migration, "
            f"immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; "
            "this is not a large-library, WAN or production claim."
        )
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    return {
        "schema": "phase11-disposable-benchmark-v1",
        "manifest": {
            "source_revision": options.source_revision,
            "fixture": {
                "id": "synthetic-phase8-takeout-64m", "kind": "synthetic",
                "license": "CC0-1.0", "manifest_sha256": sha256(options.fixture_manifest),
                "corpus_sha256": corpus_sha256(options.source), "assets": 8,
                "media_bytes": 67_108_864, "metadata_updates": 8,
            },
            "tools": {
                "immich_rs": {"version": version(options.immich_rs), "sha256": sha256(options.immich_rs)},
                "immich_go": {**baseline["oracle"], **baseline["artifacts"]["linux_x86_64"], "sha256": sha256(options.oracle)},
            },
            "environment": {
                "system": platform.system(), "kernel": platform.release(),
                "architecture": platform.machine(), "logical_cpus": os.cpu_count(),
                "locale": "C.UTF-8", "timezone": "UTC", "hostname": "<REDACTED_HOST>",
            },
            "methodology": {
                "samples": options.samples, "warmups": options.warmups,
                "pairing": "fresh isolated source and destination owner per tool on two disposable servers",
                "order": "alternating within pairs", "concurrency": 1,
                "scope": "complete inventory, plan and migration; seed, setup and probes excluded",
                "percentiles": "nearest-rank p95 over retained samples",
            },
        },
        "raw_samples": raw, "aggregate": aggregates, "claims": [claim],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-endpoint", required=True)
    parser.add_argument("--destination-endpoint", required=True)
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
        print(f"real migration benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
