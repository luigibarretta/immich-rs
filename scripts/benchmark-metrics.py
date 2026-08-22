#!/usr/bin/env python3
"""Collect bounded process metrics without buffering command output in memory."""

from __future__ import annotations

import os
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

try:
    import resource
except ModuleNotFoundError:  # pragma: no cover - exercised on Windows
    resource = None

if os.name == "nt":
    import ctypes
    from ctypes import wintypes

    _PROCESS_QUERY_INFORMATION = 0x0400
    _PROCESS_VM_READ = 0x0010
    _KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _PSAPI = ctypes.WinDLL("psapi", use_last_error=True)

    class _ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    class _IoCounters(ctypes.Structure):
        _fields_ = [
            ("ReadOperationCount", ctypes.c_ulonglong),
            ("WriteOperationCount", ctypes.c_ulonglong),
            ("OtherOperationCount", ctypes.c_ulonglong),
            ("ReadTransferCount", ctypes.c_ulonglong),
            ("WriteTransferCount", ctypes.c_ulonglong),
            ("OtherTransferCount", ctypes.c_ulonglong),
        ]

    _KERNEL32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    _KERNEL32.OpenProcess.restype = wintypes.HANDLE
    _KERNEL32.CloseHandle.argtypes = [wintypes.HANDLE]
    _KERNEL32.CloseHandle.restype = wintypes.BOOL
    _KERNEL32.GetProcessHandleCount.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    _KERNEL32.GetProcessHandleCount.restype = wintypes.BOOL
    _KERNEL32.GetProcessIoCounters.argtypes = [wintypes.HANDLE, ctypes.POINTER(_IoCounters)]
    _KERNEL32.GetProcessIoCounters.restype = wintypes.BOOL
    _KERNEL32.GetProcessTimes.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(wintypes.FILETIME),
        ctypes.POINTER(wintypes.FILETIME),
        ctypes.POINTER(wintypes.FILETIME),
        ctypes.POINTER(wintypes.FILETIME),
    ]
    _KERNEL32.GetProcessTimes.restype = wintypes.BOOL
    _PSAPI.GetProcessMemoryInfo.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(_ProcessMemoryCounters),
        wintypes.DWORD,
    ]
    _PSAPI.GetProcessMemoryInfo.restype = wintypes.BOOL


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


def _sample_procfs_process(pid: int, peaks: dict[str, Any]) -> None:
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


def _filetime_seconds(value: Any) -> float:
    ticks = (value.dwHighDateTime << 32) | value.dwLowDateTime
    return ticks / 10_000_000


def _sample_windows_process(pid: int, peaks: dict[str, Any]) -> None:
    handle = _KERNEL32.OpenProcess(
        _PROCESS_QUERY_INFORMATION | _PROCESS_VM_READ, False, pid
    )
    if not handle:
        return
    try:
        memory = _ProcessMemoryCounters()
        memory.cb = ctypes.sizeof(memory)
        if _PSAPI.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
            peaks["peak_rss_bytes"] = max(
                peaks["peak_rss_bytes"], int(memory.WorkingSetSize)
            )
        handle_count = wintypes.DWORD()
        if _KERNEL32.GetProcessHandleCount(handle, ctypes.byref(handle_count)):
            peaks["peak_open_file_descriptors"] = max(
                peaks["peak_open_file_descriptors"], int(handle_count.value)
            )
        process_io = _IoCounters()
        if _KERNEL32.GetProcessIoCounters(handle, ctypes.byref(process_io)):
            peaks["characters_read"] = max(
                peaks["characters_read"], int(process_io.ReadTransferCount)
            )
            peaks["characters_written"] = max(
                peaks["characters_written"], int(process_io.WriteTransferCount)
            )
        creation, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
        if _KERNEL32.GetProcessTimes(
            handle,
            ctypes.byref(creation),
            ctypes.byref(exited),
            ctypes.byref(kernel),
            ctypes.byref(user),
        ):
            peaks["user_cpu_seconds"] = max(
                peaks["user_cpu_seconds"], _filetime_seconds(user)
            )
            peaks["system_cpu_seconds"] = max(
                peaks["system_cpu_seconds"], _filetime_seconds(kernel)
            )
    finally:
        _KERNEL32.CloseHandle(handle)


def _sample_process(pid: int, peaks: dict[str, Any]) -> None:
    if os.name == "nt":
        _sample_windows_process(pid, peaks)
    else:
        _sample_procfs_process(pid, peaks)


def _child_cpu_usage() -> tuple[float, float]:
    if resource is None:
        return 0.0, 0.0
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_utime, usage.ru_stime


def _captured_text(handle: Any, label: str) -> str:
    handle.seek(0)
    data = handle.read(MAX_CAPTURE_BYTES + 1)
    if len(data) > MAX_CAPTURE_BYTES:
        raise BenchmarkError(f"{label} exceeded the benchmark capture limit")
    return data.decode("utf-8", errors="replace")


def _process_exited(process: subprocess.Popen[Any]) -> bool:
    waitid = getattr(os, "waitid", None)
    constants = tuple(
        getattr(os, name, None) for name in ("P_PID", "WEXITED", "WNOHANG", "WNOWAIT")
    )
    if callable(waitid) and all(value is not None for value in constants):
        pid_type, exited, no_hang, no_reap = constants
        try:
            return waitid(pid_type, process.pid, exited | no_hang | no_reap) is not None
        except (ChildProcessError, OSError):
            pass
    return process.poll() is not None


def run_command(
    command: list[str],
    cwd: Path,
    environment: dict[str, str],
    timeout_seconds: int,
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run one process, sampling RSS, descriptors and procfs I/O until exit."""
    usage_before = _child_cpu_usage()
    peaks = {
        "user_cpu_seconds": 0.0,
        "system_cpu_seconds": 0.0,
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
            if _process_exited(process):
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
        usage_after = _child_cpu_usage()
        stdout = _captured_text(stdout_handle, "stdout")
        stderr = _captured_text(stderr_handle, "stderr")
    completed = subprocess.CompletedProcess(command, return_code, stdout, stderr)
    if resource is not None:
        peaks["user_cpu_seconds"] = max(0.0, usage_after[0] - usage_before[0])
        peaks["system_cpu_seconds"] = max(0.0, usage_after[1] - usage_before[1])
    metrics = {
        "wall_time_seconds": (finished - started) / 1_000_000_000,
        **peaks,
    }
    return completed, metrics
