#!/usr/bin/env python3
"""Benchmark paired Apple Photos or Picasa imports on one disposable Immich."""

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
    """A paired source benchmark invariant failed."""


def load_metrics() -> ModuleType:
    path = ROOT / "scripts/benchmark-metrics.py"
    spec = importlib.util.spec_from_file_location("source_import_metrics", path)
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
        opener = request.build_opener(request.ProxyHandler({}))
        with opener.open(query, timeout=30) as response:
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


def create_account(endpoint: str, admin: str, run_id: str, index: int, tool: str) -> tuple[str, str]:
    email = f"synthetic-{run_id}-{index}-{tool}@example.invalid"
    password = "Synthetic-only-" + secrets.token_urlsafe(24)
    api(endpoint, "/api/admin/users", "POST", {
        "email": email, "name": "Synthetic Source Benchmark",
        "password": password, "shouldChangePassword": False,
    }, admin)
    login = api(endpoint, "/api/auth/login", "POST", {"email": email, "password": password})
    token = login.get("accessToken") if isinstance(login, dict) else None
    if not isinstance(token, str) or not token:
        raise BenchmarkError("disposable login omitted its access token")
    key = api(endpoint, "/api/api-keys", "POST", {
        "name": "immich-rs paired source benchmark",
        "permissions": ["asset.read", "asset.statistics", "asset.upload", "server.about", "user.read"],
    }, token)
    secret = key.get("secret") if isinstance(key, dict) else None
    if not isinstance(secret, str) or not secret:
        raise BenchmarkError("disposable API key creation failed")
    return token, secret


def load_fixture(path: Path, source: Path) -> dict[str, Any]:
    fixture = json.loads(path.read_text(encoding="utf-8"))
    expected = {
        "media_assets": 8, "media_bytes": 67_108_864, "metadata_updates": 0,
        "album_mutations": 0, "concurrency": 1,
    }
    provenance = fixture.get("provenance") if isinstance(fixture, dict) else None
    if fixture.get("schema") != "fixture-manifest-v1" or fixture.get("benchmark") != expected:
        raise BenchmarkError("source benchmark fixture contract drifted")
    if provenance != {
        "kind": "synthetic", "generator": "scripts/materialize-fixture.py",
        "generator_version": "1", "license": "CC0-1.0",
    }:
        raise BenchmarkError("source benchmark provenance drifted")
    files = [path for path in source.rglob("*") if path.is_file()]
    if len(files) != 8 or any(path.stat().st_size != 8_388_608 for path in files):
        raise BenchmarkError("materialized source benchmark corpus drifted")
    return fixture


def environment(workspace: Path, api_key: str | None = None) -> dict[str, str]:
    home = workspace / "home"
    home.mkdir(exist_ok=True)
    selected = {
        "HOME": str(home), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "NO_COLOR": "1", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TZ": "UTC",
    }
    if api_key is not None:
        selected["IMMICH_RS_API_KEY"] = api_key
    return selected


def source_options(adapter: str) -> list[str]:
    return ["--album-mode", "none"] if adapter == "apple-photos" else [
        "--album-mode", "none", "--no-picasa-albums", "--no-filename-date",
    ]


def combine(values: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        field: sum(item[field] for item in values) if field in SUM_METRICS
        else max(item[field] for item in values)
        for field in METRICS
    }


def run_rust(options: argparse.Namespace, workspace: Path, key: str, metrics: ModuleType) -> dict[str, Any]:
    plan_path = workspace / "plan.json"
    checkpoint = workspace / "checkpoint.sqlite"
    source_args = source_options(options.adapter)
    planned, plan_metrics = metrics.run_command([
        str(options.immich_rs), "plan", "upload", options.adapter,
        "--server", options.endpoint, *source_args, str(options.source),
    ], workspace, environment(workspace, key), 300)
    if planned.returncode != 0:
        raise BenchmarkError("immich-rs source planning failed")
    plan = json.loads(planned.stdout)
    if plan.get("summary") != {
        "operations": 8, "media_bytes": 67_108_864, "xmp_sidecars": 0,
        "live_photo_pairs": 0, "max_mutations": 8,
    }:
        raise BenchmarkError("immich-rs source plan counters drifted")
    plan_path.write_text(planned.stdout, encoding="utf-8")
    plan_path.chmod(0o600)
    applied, apply_metrics = metrics.run_command([
        str(options.immich_rs), "apply", "upload", "--server", options.endpoint,
        "--plan", str(plan_path), "--source", str(options.source),
        "--checkpoint", str(checkpoint), *source_args,
    ], workspace, environment(workspace, key), 300)
    if applied.returncode != 0:
        raise BenchmarkError("immich-rs source apply failed")
    report = json.loads(applied.stdout)
    if report.get("created") != 8 or report.get("retried") != 0:
        raise BenchmarkError("immich-rs source report drifted")
    return combine([plan_metrics, apply_metrics])


def run_go(options: argparse.Namespace, workspace: Path, key: str, metrics: ModuleType) -> dict[str, Any]:
    command = "from-icloud" if options.adapter == "apple-photos" else "from-picasa"
    specific = ["--folder-as-album", "NONE"] if options.adapter == "apple-photos" else [
        "--album-picasa=false", "--folder-as-album", "NONE", "--date-from-name=false",
    ]
    completed, measured = metrics.run_command([
        str(options.oracle), "upload", command, "--server", options.endpoint,
        "--api-key", key, "--device-uuid", f"synthetic-{options.adapter}-benchmark",
        "--no-ui", "--log-level", "ERROR", "--pause-immich-jobs=false",
        "--concurrent-tasks", "1", "--on-errors", "stop", *specific, str(options.source),
    ], workspace, environment(workspace), 300)
    if completed.returncode != 0:
        details = completed.stdout + "\n" + completed.stderr
        for sensitive in (key, options.endpoint, str(options.source), str(workspace)):
            details = details.replace(sensitive, "<REDACTED>")
        raise BenchmarkError(f"immich-go source import failed: {details[-1000:]}")
    return measured


def outcome(endpoint: str, token: str) -> dict[str, int]:
    statistics_value = api(endpoint, "/api/assets/statistics", token=token)
    search = api(endpoint, "/api/search/metadata", "POST", {"size": 100}, token)
    items = search.get("assets", {}).get("items") if isinstance(search, dict) else None
    if not isinstance(items, list) or statistics_value.get("total") != 8 or len(items) != 8:
        raise BenchmarkError("benchmark server outcome drifted")
    return {"assets": 8, "media_bytes": 67_108_864, "retries": 0}


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
    if options.adapter not in {"apple-photos", "picasa"}:
        raise BenchmarkError("unsupported source adapter")
    if not re.fullmatch(r"http://127\.0\.0\.1:[0-9]+", options.endpoint):
        raise BenchmarkError("benchmark endpoint must be disposable loopback")
    if re.fullmatch(r"[0-9a-f]{40}", options.source_revision) is None:
        raise BenchmarkError("source revision must be an exact SHA")
    if not 2 <= options.samples <= 10 or not 0 <= options.warmups <= 2:
        raise BenchmarkError("sample or warmup count is outside its bound")
    admin = os.environ.get("IMMICH_RS_BENCHMARK_ADMIN_TOKEN")
    if not admin:
        raise BenchmarkError("disposable admin token is missing")
    metrics = load_metrics()
    fixture = load_fixture(options.fixture_manifest, options.source)
    raw = []
    for index in range(options.samples + options.warmups):
        order = ("immich_rs", "immich_go") if index % 2 == 0 else ("immich_go", "immich_rs")
        pair: dict[str, Any] = {"execution_order": f"{order[0].replace('_', '-')}-first"}
        for tool in order:
            workspace = options.workspace / f"pair-{index}-{tool}"
            workspace.mkdir(parents=True)
            token, key = create_account(options.endpoint, admin, options.run_id, index, tool)
            measured = run_rust(options, workspace, key, metrics) if tool == "immich_rs" else run_go(options, workspace, key, metrics)
            measured["operations"] = outcome(options.endpoint, token)
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
            f"On this exact 67,108,864-byte eight-asset synthetic {options.adapter} import, "
            f"immich-rs median wall time was {improvement:.1f}% lower than immich-go v0.32.0; "
            "this is not a large-library, WAN or production claim."
        )
    baseline = tomllib.loads(BASELINE.read_text(encoding="utf-8"))
    return {
        "schema": "source-import-benchmark-v1", "adapter": options.adapter,
        "manifest": {
            "source_revision": options.source_revision,
            "fixture": {
                "id": fixture["fixture_id"], "kind": "synthetic", "license": "CC0-1.0",
                "manifest_sha256": sha256(options.fixture_manifest),
                "corpus_sha256": corpus_sha256(options.source), **fixture["benchmark"],
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
                "pairing": "fresh isolated owner per tool on one disposable server and one corpus",
                "order": "alternating within pairs", "concurrency": 1,
                "scope": "complete plan plus import; setup and probes excluded",
                "compatibility_intersection": "uploads only; albums and metadata disabled",
                "percentiles": "nearest-rank p95 over retained samples",
            },
        },
        "raw_samples": raw, "aggregate": aggregates, "claims": [claim],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", required=True)
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
    options = parser.parse_args()
    try:
        report = run(options)
        options.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (BenchmarkError, OSError, UnicodeError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"source import benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
