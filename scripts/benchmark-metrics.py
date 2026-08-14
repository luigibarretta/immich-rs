#!/usr/bin/env python3
"""Collect bounded Linux process metrics without buffering command output in memory."""

from __future__ import annotations

import resource
import os
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any


MAX_CAPTURE_BYTES = 4 * 1024 * 1024
SAMPLE_SECONDS = 0.002


class BenchmarkError(RuntimeError):
    """A benchmark process could not be measured safely."""


def _read_key_values(path: Path) -> dict[str, int]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError):
        return {}
    values: dict[str, int] = {}
    for line in lines:
        key, separator, raw = line.partition(":")
        if not separator:
            continue
        fields = raw.strip().split(maxsplit=1)
        if not fields:
            continue
        first = fields[0]
        if first.isdigit():
            values[key] = int(first)
    return values


def _sample_process(pid: int, peaks: dict[str, int]) -> None:
    process_root = Path("/proc") / str(pid)
    status = _read_key_values(process_root / "status")
    peaks["peak_rss_bytes"] = max(peaks["peak_rss_bytes"], status.get("VmRSS", 0) * 1024)
    try:
        descriptors = sum(1 for _ in (process_root / "fd").iterdir())
    except OSError:
        descriptors = 0
    peaks["peak_open_file_descriptors"] = max(peaks["peak_open_file_descriptors"], descriptors)
    process_io = _read_key_values(process_root / "io")
    for source, destination in (
        ("rchar", "characters_read"),
        ("wchar", "characters_written"),
        ("read_bytes", "storage_bytes_read"),
        ("write_bytes", "storage_bytes_written"),
    ):
        peaks[destination] = max(peaks[destination], process_io.get(source, 0))


def _captured_text(handle: Any, label: str) -> str:
    handle.seek(0)
    data = handle.read(MAX_CAPTURE_BYTES + 1)
    if len(data) > MAX_CAPTURE_BYTES:
        raise BenchmarkError(f"{label} exceeded the benchmark capture limit")
    return data.decode("utf-8", errors="replace")


def run_command(
    command: list[str],
    cwd: Path,
    environment: dict[str, str],
    timeout_seconds: int,
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run one process, sampling RSS, descriptors and procfs I/O until exit."""
    usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
    peaks = {
        "peak_rss_bytes": 0,
        "peak_open_file_descriptors": 0,
        "characters_read": 0,
        "characters_written": 0,
        "storage_bytes_read": 0,
        "storage_bytes_written": 0,
    }
    with tempfile.TemporaryFile() as stdout_handle, tempfile.TemporaryFile() as stderr_handle:
        started = time.monotonic_ns()
        try:
            process = subprocess.Popen(
                command,
                cwd=cwd,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=stdout_handle,
                stderr=stderr_handle,
            )
        except OSError as error:
            raise BenchmarkError(f"cannot start benchmark process: {error}") from error
        deadline = time.monotonic() + timeout_seconds
        while True:
            _sample_process(process.pid, peaks)
            completed = os.waitid(
                os.P_PID,
                process.pid,
                os.WEXITED | os.WNOHANG | os.WNOWAIT,
            )
            if completed is not None:
                break
            if time.monotonic() >= deadline:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
                raise subprocess.TimeoutExpired(command, timeout_seconds)
            time.sleep(SAMPLE_SECONDS)
        _sample_process(process.pid, peaks)
        return_code = process.wait()
        finished = time.monotonic_ns()
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
        stdout = _captured_text(stdout_handle, "stdout")
        stderr = _captured_text(stderr_handle, "stderr")
    completed = subprocess.CompletedProcess(command, return_code, stdout, stderr)
    metrics = {
        "wall_time_seconds": (finished - started) / 1_000_000_000,
        "user_cpu_seconds": max(0.0, usage_after.ru_utime - usage_before.ru_utime),
        "system_cpu_seconds": max(0.0, usage_after.ru_stime - usage_before.ru_stime),
        **peaks,
    }
    return completed, metrics
