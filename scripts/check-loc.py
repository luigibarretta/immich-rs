#!/usr/bin/env python3
"""Fail closed when maintained source files exceed the documented LOC policy."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


DEFAULT_IGNORED_DIRS = {
    ".git",
    ".next",
    ".turbo",
    "__pycache__",
    "coverage",
    "dist",
    "node_modules",
    "target",
}
EXPECTED_KEYS = {
    "maxLines",
    "allowedGrowthLines",
    "roots",
    "extensions",
    "ignore",
    "allow",
}


def parse_args() -> argparse.Namespace:
    repository = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=repository)
    parser.add_argument("--policy", type=Path, default=Path("scripts/loc-baseline.json"))
    return parser.parse_args()


def repo_path(value: Any, label: str, errors: list[str]) -> str:
    if not isinstance(value, str) or not value:
        errors.append(f"{label} must be a non-empty string")
        return ""
    path = Path(value)
    if path.is_absolute() or ".." in path.parts or "\\" in value:
        errors.append(f"{label} must be a normalized repository-relative path")
        return ""
    return path.as_posix()


def integer(value: Any, label: str, minimum: int, errors: list[str]) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        errors.append(f"{label} must be an integer >= {minimum}")
        return minimum
    return value


def mapping(value: Any, label: str, errors: list[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        errors.append(f"{label} must be an object")
        return {}
    return value


def string_list(value: Any, label: str, errors: list[str]) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        errors.append(f"{label} must be a non-empty-string array")
        return []
    return value


def load_policy(root: Path, policy_path: Path) -> tuple[dict[str, Any], list[str]]:
    errors: list[str] = []
    resolved = policy_path if policy_path.is_absolute() else root / policy_path
    try:
        policy = json.loads(resolved.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return {}, [f"cannot read LOC policy: {error}"]
    if not isinstance(policy, dict):
        return {}, ["LOC policy must be a JSON object"]
    unknown = sorted(set(policy) - EXPECTED_KEYS)
    errors.extend(f"unknown LOC policy key: {key}" for key in unknown)
    return policy, errors


def matches_prefix(relative: str, candidates: dict[str, Any]) -> str | None:
    return next(
        (candidate for candidate in candidates if relative == candidate or relative.startswith(candidate + "/")),
        None,
    )


def source_files(
    root: Path,
    roots: list[str],
    extensions: set[str],
    ignored: dict[str, Any],
    errors: list[str],
) -> tuple[list[Path], set[str]]:
    files: list[Path] = []
    seen_ignored: set[str] = set()
    for source_root in roots:
        directory = root / source_root
        if not directory.is_dir():
            errors.append(f"LOC root does not exist: {source_root}")
            continue
        for path in directory.rglob("*"):
            relative = path.relative_to(root).as_posix()
            ignored_match = matches_prefix(relative, ignored)
            if ignored_match:
                seen_ignored.add(ignored_match)
                continue
            if any(part in DEFAULT_IGNORED_DIRS for part in path.relative_to(root).parts):
                continue
            if path.is_file() and not path.is_symlink() and path.suffix in extensions:
                files.append(path)
    return sorted(set(files)), seen_ignored


def line_count(path: Path) -> int:
    text = path.read_text(encoding="utf-8")
    return len(text.splitlines())


def main() -> int:
    args = parse_args()
    root = args.root.resolve()
    policy, errors = load_policy(root, args.policy)
    max_lines = integer(policy.get("maxLines"), "maxLines", 1, errors)
    growth = integer(policy.get("allowedGrowthLines"), "allowedGrowthLines", 0, errors)
    roots = [repo_path(item, "root", errors) for item in string_list(policy.get("roots"), "roots", errors)]
    extensions = set(string_list(policy.get("extensions"), "extensions", errors))
    if any(not extension.startswith(".") for extension in extensions):
        errors.append("extensions must start with a dot")
    ignored = mapping(policy.get("ignore"), "ignore", errors)
    allowed = mapping(policy.get("allow"), "allow", errors)
    for label, entries in (("ignore", ignored), ("allow", allowed)):
        for relative, entry in entries.items():
            repo_path(relative, f"{label} entry", errors)
            if not isinstance(entry, dict) or not isinstance(entry.get("reason"), str) or not entry["reason"].strip():
                errors.append(f"{label} entry {relative} needs a non-empty reason")
    files, seen_ignored = source_files(root, roots, extensions, ignored, errors)
    violations: list[str] = []
    stale: list[str] = []
    seen_allowed: set[str] = set()
    for path in files:
        relative = path.relative_to(root).as_posix()
        lines = line_count(path)
        baseline = allowed.get(relative)
        if baseline is None:
            if lines > max_lines:
                violations.append(f"{relative} has {lines} LOC (max {max_lines})")
            continue
        seen_allowed.add(relative)
        baseline_lines = integer(baseline.get("linesAtBaseline"), f"{relative}.linesAtBaseline", 1, errors)
        if baseline_lines <= max_lines:
            errors.append(f"{relative}.linesAtBaseline must exceed maxLines")
        if lines <= max_lines:
            stale.append(f"{relative} is below the limit; remove its allowance")
        elif lines > baseline_lines + growth:
            violations.append(f"{relative} grew to {lines} LOC (baseline {baseline_lines}, growth {growth})")
    stale.extend(f"missing allowed path: {path}" for path in set(allowed) - seen_allowed)
    stale.extend(f"unused ignored path: {path}" for path in set(ignored) - seen_ignored)
    if errors or violations or stale:
        for heading, findings in (("invalid policy", errors), ("LOC violations", violations), ("stale baseline", stale)):
            if findings:
                print(f"{heading}:", file=sys.stderr)
                for finding in findings:
                    print(f"- {finding}", file=sys.stderr)
        return 1
    print(
        f"LOC guard passed: {len(files)} files, max {max_lines} LOC, "
        f"{len(allowed)} allowances, growth budget {growth} LOC."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
