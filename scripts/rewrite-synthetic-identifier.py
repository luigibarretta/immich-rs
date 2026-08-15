#!/usr/bin/env python3
"""Replace one fixed-size synthetic identifier without buffering its media."""

from __future__ import annotations

import argparse
import sys


CHUNK_BYTES = 65_536


def identifier(value: str) -> bytes:
    try:
        encoded = value.encode("ascii")
    except UnicodeEncodeError as failure:
        raise ValueError("identifiers must be ASCII") from failure
    if not encoded or len(encoded) > 128:
        raise ValueError("identifiers must contain 1..128 bytes")
    return encoded


def rewrite(source: bytes, replacement: bytes) -> int:
    pending = b""
    replacements = 0
    while chunk := sys.stdin.buffer.read(CHUNK_BYTES):
        pending += chunk
        while (position := pending.find(source)) >= 0:
            sys.stdout.buffer.write(pending[:position])
            sys.stdout.buffer.write(replacement)
            pending = pending[position + len(source) :]
            replacements += 1
        safe = max(0, len(pending) - len(source) + 1)
        sys.stdout.buffer.write(pending[:safe])
        pending = pending[safe:]
    sys.stdout.buffer.write(pending)
    return replacements


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from-identifier", required=True)
    parser.add_argument("--to-identifier", required=True)
    arguments = parser.parse_args()
    try:
        source = identifier(arguments.from_identifier)
        replacement = identifier(arguments.to_identifier)
    except ValueError as failure:
        parser.error(str(failure))
    if len(source) != len(replacement) or source == replacement:
        parser.error("identifiers must be distinct and have equal encoded lengths")
    replacements = rewrite(source, replacement)
    if replacements != 1:
        print(f"expected one synthetic identifier, replaced {replacements}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
