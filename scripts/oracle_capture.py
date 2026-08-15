#!/usr/bin/env python3
"""Bound and normalize observable immich-go text without interpreting source code."""

from __future__ import annotations

from pathlib import Path
import re
from typing import Any
import unicodedata

ANSI_ESCAPE = re.compile(r"\x1b(?:[@-_][0-?]*[ -/]*[@-~]|\[[0-?]*[ -/]*[@-~])")
UUID = re.compile(
    r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b"
)
RFC3339 = re.compile(r"\b\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+Z?\b")
CLOCK_TIME = re.compile(r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b")
LOG_TIMESTAMP = re.compile(r"(?<!\d)\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}(?!\d)")
COORDINATE_FIELDS = re.compile(r"(\s+file\.(?:Latitude|Longitude)=)[^\s]+")
LOGGED_API_KEY = re.compile(r"--api-key=[^\s]+ origin=cli")


def normalize_text(
    text: str, fixture_root: Path, workspace_root: Path, server_url: str
) -> list[str]:
    """Remove volatile or secret values and normalize line ordering inputs."""
    normalized = ANSI_ESCAPE.sub("", text).replace(str(fixture_root), "<FIXTURE_ROOT>")
    normalized = normalized.replace(str(workspace_root), "<WORKSPACE>").replace(
        server_url, "<MOCK_SERVER>"
    )
    normalized = UUID.sub("<ID>", normalized)
    normalized = RFC3339.sub("<TIMESTAMP>", normalized)
    normalized = CLOCK_TIME.sub("<TIME>", normalized)
    normalized = LOG_TIMESTAMP.sub("<TIMESTAMP>", normalized)
    normalized = normalized.replace("synthetic-oracle-key", "<SYNTHETIC_API_KEY>")
    normalized = normalized.replace("synthetic-user@example.invalid", "<SYNTHETIC_USER>")
    normalized = COORDINATE_FIELDS.sub(r"\1<COORDINATE>", normalized)
    normalized = LOGGED_API_KEY.sub("--api-key=<REDACTED> origin=cli", normalized)
    normalized = unicodedata.normalize("NFC", normalized)
    return [
        line.rstrip()
        for line in normalized.replace("\r", "\n").splitlines()
        if line.strip()
    ]


def captured_logs(
    process_home: Path, fixture_root: Path, workspace_root: Path, server_url: str
) -> list[dict[str, Any]]:
    """Capture only bounded semantic log lines from the isolated oracle home."""
    log_root = process_home / ".cache" / "immich-go"
    if not log_root.exists():
        return []
    captured = []
    total_bytes = 0
    markers = (
        " discovered ",
        " uploaded successfully ",
        " discarded local duplicate ",
        " metadata updated ",
        " associated metadata ",
        " added to album ",
        " missing metadata ",
        " stacked ",
        "JSON file detected",
        "Total Assets:",
        "  Processed:",
        "  Discarded:",
        "  Errors:",
        "  Pending:",
    )
    for path in sorted(log_root.glob("*.log")):
        data = path.read_bytes()
        total_bytes += len(data)
        if total_bytes > 4 * 1024 * 1024:
            raise ValueError("oracle logs exceeded the synthetic capture limit")
        normalized = normalize_text(
            data.decode("utf-8", errors="replace"), fixture_root, workspace_root, server_url
        )
        captured.append(
            {
                "name": LOG_TIMESTAMP.sub("<TIMESTAMP>", path.name),
                "lines": sorted(line for line in normalized if any(marker in line for marker in markers)),
            }
        )
    return captured
