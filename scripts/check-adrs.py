#!/usr/bin/env python3
"""Validate the repository ADR inventory without requiring POSIX tools."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys


ADR_NAME = re.compile(r"ADR-[0-9]{4}-.+\.md\Z")
VALID_STATUS = re.compile(r"^- Status: (Accepted|Proposed|Superseded|Rejected)$", re.MULTILINE)
REQUIRED_HEADINGS = ("Context", "Decision", "Consequences", "Verification")


def validate(root: Path, minimum_count: int = 18) -> list[str]:
    adr_dir = root / "docs" / "adr"
    if not adr_dir.is_dir():
        return [f"ADR directory does not exist: {adr_dir}"]
    files = sorted(path for path in adr_dir.iterdir() if path.is_file() and ADR_NAME.fullmatch(path.name))
    errors = []
    if len(files) < minimum_count:
        errors.append(f"expected at least {minimum_count} ADRs, found {len(files)}")
    for path in files:
        try:
            document = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"{path}: cannot read UTF-8 document: {error}")
            continue
        if VALID_STATUS.search(document) is None:
            errors.append(f"{path}: missing valid status")
        lines = set(document.splitlines())
        for heading in REQUIRED_HEADINGS:
            if f"## {heading}" not in lines:
                errors.append(f"{path}: missing ## {heading}")
    return errors


def parse_args() -> argparse.Namespace:
    repository = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=repository)
    parser.add_argument("--minimum-count", type=int, default=18)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.minimum_count < 1:
        print("ADR check failed: minimum count must be positive", file=sys.stderr)
        return 1
    errors = validate(args.root.resolve(), args.minimum_count)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("ADR checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
