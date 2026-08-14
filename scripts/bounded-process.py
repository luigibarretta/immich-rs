#!/usr/bin/env python3
"""Run a subprocess with bounded stdout and stderr capture."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile


MAX_CAPTURE_BYTES = 4 * 1024 * 1024


class CaptureError(ValueError):
    """A subprocess exceeded a capture limit or could not stop cleanly."""


def _text(handle, label: str) -> str:
    handle.seek(0)
    data = handle.read(MAX_CAPTURE_BYTES + 1)
    if len(data) > MAX_CAPTURE_BYTES:
        raise CaptureError(f"oracle {label} exceeded {MAX_CAPTURE_BYTES} bytes")
    return data.decode("utf-8", errors="replace")


def run_command(
    command: list[str],
    cwd: Path,
    environment: dict[str, str],
    timeout_seconds: int,
) -> subprocess.CompletedProcess[str]:
    """Capture text through temporary files so pipes cannot grow or deadlock."""
    with tempfile.TemporaryFile() as stdout_handle, tempfile.TemporaryFile() as stderr_handle:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=stdout_handle,
            stderr=stderr_handle,
        )
        try:
            return_code = process.wait(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
            raise
        stdout = _text(stdout_handle, "stdout")
        stderr = _text(stderr_handle, "stderr")
    return subprocess.CompletedProcess(command, return_code, stdout, stderr)
