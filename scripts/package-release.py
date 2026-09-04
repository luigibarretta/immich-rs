#!/usr/bin/env python3
"""Create one deterministic native release archive with SBOM and provenance."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import zipfile
from typing import BinaryIO

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": ("ubuntu-24.04", "immich-rs", "tar.gz"),
    "aarch64-unknown-linux-gnu": ("ubuntu-24.04-arm", "immich-rs", "tar.gz"),
    "x86_64-apple-darwin": ("macos-15-intel", "immich-rs", "tar.gz"),
    "aarch64-apple-darwin": ("macos-15", "immich-rs", "tar.gz"),
    "x86_64-pc-windows-msvc": ("windows-2025", "immich-rs.exe", "zip"),
}
DOCUMENTS = (
    "LICENSE", "NOTICE.md", "README.md", "SECURITY.md", "CHANGELOG.md",
    "docs/migration-from-immich-go.md", "docs/compatibility/phase1-folder.md",
    "docs/compatibility/phase2-folder-upload.md",
    "docs/compatibility/phase3-google-takeout.md",
    "docs/compatibility/phase4-apple-photos.md",
    "docs/compatibility/phase5-archive.md",
    "docs/compatibility/phase7-production-https.md",
    "docs/compatibility/phase8-google-takeout-import.md",
)


class PackageError(RuntimeError):
    """A native release package could not be reproduced safely."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def run(arguments: list[str]) -> str:
    try:
        return subprocess.run(
            arguments, cwd=ROOT, check=True, capture_output=True, text=True, timeout=60
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError) as error:
        raise PackageError(f"cannot run {arguments[0]}") from error


def host_target() -> str:
    for line in run(["rustc", "-vV"]).splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    raise PackageError("rustc host target is unavailable")


def workspace_version() -> str:
    try:
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        value = manifest["workspace"]["package"]["version"]
    except (OSError, UnicodeError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        raise PackageError("workspace version is unavailable") from error
    if not isinstance(value, str):
        raise PackageError("workspace version is invalid")
    return value


def validate_sbom(path: Path) -> None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PackageError("CycloneDX SBOM is unreadable") from error
    if not isinstance(value, dict) or value.get("bomFormat") != "CycloneDX":
        raise PackageError("CycloneDX JSON SBOM is invalid")


def tar_entry(name: str, size: int, epoch: int, executable: bool) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.size = size
    info.mtime = epoch
    info.mode = 0o755 if executable else 0o644
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    return info


def add_tar_path(
    archive: tarfile.TarFile, source: Path, name: str, epoch: int, executable: bool
) -> None:
    with source.open("rb") as handle:
        archive.addfile(tar_entry(name, source.stat().st_size, epoch, executable), handle)


def zip_entry(name: str, epoch: int, executable: bool) -> zipfile.ZipInfo:
    timestamp = time.gmtime(max(epoch, 315_532_800))[:6]
    info = zipfile.ZipInfo(name, timestamp)
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = (0o755 if executable else 0o644) << 16
    return info


def add_zip_path(
    archive: zipfile.ZipFile, source: Path, name: str, epoch: int, executable: bool
) -> None:
    with source.open("rb") as input_handle, archive.open(
        zip_entry(name, epoch, executable), "w"
    ) as output_handle:
        shutil.copyfileobj(input_handle, output_handle, length=1_048_576)


def provenance(
    target: str, runner: str, version: str, revision: str, binary: Path, sbom: Path, epoch: int
) -> bytes:
    value = {
        "schema": "immich-rs-build-provenance-v1",
        "source_revision": revision,
        "source_date_epoch": epoch,
        "version": version,
        "target": target,
        "runner_class": runner,
        "rustc": run(["rustc", "--version"]),
        "inputs": {"cargo_lock_sha256": sha256(ROOT / "Cargo.lock")},
        "subjects": {
            "binary_sha256": sha256(binary),
            "sbom_sha256": sha256(sbom),
        },
    }
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def package(arguments: argparse.Namespace) -> list[Path]:
    if arguments.target not in TARGETS:
        raise PackageError("unsupported release target")
    runner, binary_name, extension = TARGETS[arguments.target]
    if arguments.runner != runner or host_target() != arguments.target:
        raise PackageError("native runner label or rustc host target mismatch")
    if not arguments.binary.is_file() or arguments.binary.name != binary_name:
        raise PackageError("native release binary is missing or misnamed")
    if arguments.binary.stat().st_size > 128 * 1_024 * 1_024:
        raise PackageError("native release binary exceeds its package bound")
    validate_sbom(arguments.sbom)
    if re.fullmatch(r"[0-9a-f]{40}", arguments.revision) is None:
        raise PackageError("source revision must be exact")
    version = workspace_version()
    if arguments.tag != f"v{version}" or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*", version):
        raise PackageError("tag and release-candidate workspace version differ")
    epoch = arguments.epoch
    if epoch is None:
        try:
            epoch = int(run(["git", "show", "-s", "--format=%ct", arguments.revision]))
        except ValueError as error:
            raise PackageError("source commit epoch is invalid") from error
    if epoch < 315_532_800:
        raise PackageError("source date epoch predates deterministic ZIP support")
    missing = [name for name in DOCUMENTS if not (ROOT / name).is_file()]
    if missing:
        raise PackageError("release package documentation is incomplete")
    arguments.output.mkdir(parents=True, exist_ok=True)
    stem = f"immich-rs-{version}-{arguments.target}"
    archive_path = arguments.output / f"{stem}.{extension}"
    sbom_path = arguments.output / f"{stem}.sbom.cdx.json"
    provenance_path = arguments.output / f"{stem}.provenance.json"
    if any(path.exists() for path in (archive_path, sbom_path, provenance_path)):
        raise PackageError("release output already exists")
    provenance_bytes = provenance(
        arguments.target, runner, version, arguments.revision,
        arguments.binary, arguments.sbom, epoch,
    )
    provenance_path.write_bytes(provenance_bytes)
    shutil.copyfile(arguments.sbom, sbom_path)
    prefix = f"immich-rs-{version}"
    entries = [(arguments.binary, f"{prefix}/{binary_name}", True)]
    entries.extend((ROOT / name, f"{prefix}/{name}", False) for name in DOCUMENTS)
    entries.extend(((sbom_path, f"{prefix}/sbom.cdx.json", False),))
    if extension == "tar.gz":
        with archive_path.open("xb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=arguments.epoch) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    for source, name, executable in entries:
                        add_tar_path(archive, source, name, epoch, executable)
                    archive.addfile(
                        tar_entry(f"{prefix}/provenance.json", len(provenance_bytes), epoch, False),
                        io.BytesIO(provenance_bytes),
                    )
    else:
        with zipfile.ZipFile(archive_path, "x") as archive:
            for source, name, executable in entries:
                add_zip_path(archive, source, name, epoch, executable)
            archive.writestr(zip_entry(f"{prefix}/provenance.json", epoch, False), provenance_bytes)
    return [archive_path, sbom_path, provenance_path]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--sbom", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--runner", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--epoch", type=int)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        outputs = package(arguments)
    except (PackageError, OSError, UnicodeError, ValueError) as error:
        print(f"release package failed: {error}", file=sys.stderr)
        return 1
    print("\n".join(str(path) for path in outputs))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
