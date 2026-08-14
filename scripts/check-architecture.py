#!/usr/bin/env python3
"""Enforce workspace dependency boundaries and the Phase-1 read-only graph."""

from __future__ import annotations

from pathlib import Path
import sys
import tomllib

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = {
    "immich-rs-cli": REPOSITORY_ROOT / "crates" / "immich-cli" / "Cargo.toml",
    "immich-rs-client": REPOSITORY_ROOT / "crates" / "immich-client" / "Cargo.toml",
    "immich-rs-core": REPOSITORY_ROOT / "crates" / "immich-core" / "Cargo.toml",
    "immich-rs-sources": REPOSITORY_ROOT / "crates" / "immich-sources" / "Cargo.toml",
}
ALLOWED_INTERNAL = {
    "immich-rs-cli": {"immich-rs-core", "immich-rs-sources"},
    "immich-rs-client": set(),
    "immich-rs-core": set(),
    "immich-rs-sources": {"immich-rs-core"},
}


def internal_dependencies(manifest: Path) -> set[str]:
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    dependencies = data.get("dependencies", {})
    if not isinstance(dependencies, dict):
        raise ValueError(f"{manifest}: dependencies must be a table")
    return {name for name in dependencies if name.startswith("immich-rs-")}


def check() -> list[str]:
    failures = []
    for package, manifest in MANIFESTS.items():
        actual = internal_dependencies(manifest)
        unexpected = actual - ALLOWED_INTERNAL[package]
        if unexpected:
            failures.append(f"{package}: forbidden internal dependencies: {sorted(unexpected)}")
    cli_dependencies = internal_dependencies(MANIFESTS["immich-rs-cli"])
    if "immich-rs-client" in cli_dependencies:
        failures.append("Phase-1 CLI dependency graph contains the Immich client")
    client_source = (REPOSITORY_ROOT / "crates" / "immich-client" / "src" / "lib.rs").read_text(encoding="utf-8")
    mutation_tokens = ("upload", "delete", "replace", "mutate", "multipart")
    if any(token in client_source.casefold() for token in mutation_tokens):
        failures.append("Phase-1 Immich client source contains a mutation capability token")
    return failures


def main() -> int:
    try:
        failures = check()
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, ValueError) as error:
        print(f"architecture check failed: {error}", file=sys.stderr)
        return 1
    if failures:
        print("architecture check failed:\n" + "\n".join(failures), file=sys.stderr)
        return 1
    print("workspace boundaries and read-only dependency graph passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
