#!/usr/bin/env python3
"""Enforce workspace dependency and Phase-2 capability boundaries."""

from __future__ import annotations

from pathlib import Path
import sys
import tomllib

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = {
    "immich-rs-cli": REPOSITORY_ROOT / "crates" / "immich-cli" / "Cargo.toml",
    "immich-rs-client": REPOSITORY_ROOT / "crates" / "immich-client" / "Cargo.toml",
    "immich-rs-core": REPOSITORY_ROOT / "crates" / "immich-core" / "Cargo.toml",
    "immich-rs-executor": REPOSITORY_ROOT / "crates" / "immich-executor" / "Cargo.toml",
    "immich-rs-sources": REPOSITORY_ROOT / "crates" / "immich-sources" / "Cargo.toml",
}
ALLOWED_INTERNAL = {
    "immich-rs-cli": {"immich-rs-core", "immich-rs-sources"},
    "immich-rs-client": {"immich-rs-core"},
    "immich-rs-core": set(),
    "immich-rs-executor": {"immich-rs-client", "immich-rs-core", "immich-rs-sources"},
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
    client_root = REPOSITORY_ROOT / "crates" / "immich-client" / "src"
    client_source = (client_root / "lib.rs").read_text(encoding="utf-8")
    upload_source = (client_root / "upload.rs").read_text(encoding="utf-8")
    read_source = (client_root / "read.rs").read_text(encoding="utf-8")
    if "ImmichReadClient" not in client_source or "ImmichUploadClient" not in client_source:
        failures.append("client does not expose separate read and upload capability types")
    if "pub fn authorize_upload" not in read_source or "NegotiatedServer" not in read_source:
        failures.append("upload capability is not restricted to an opaque probe proof")
    forbidden_mutations = ("delete(", "replace(", "put(", "patch(")
    if any(token in upload_source.casefold() for token in forbidden_mutations):
        failures.append("Phase-2 client contains an unapproved mutation primitive")
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
    print("workspace and Phase-2 capability boundaries passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
