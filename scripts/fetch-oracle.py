#!/usr/bin/env python3
"""Populate a local cache with the checksum-pinned official immich-go oracle."""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile
import tomllib
import urllib.request


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
BASELINE_PATH = REPOSITORY_ROOT / "tests" / "oracle" / "baseline.toml"
OFFICIAL_RELEASE_BASE = "https://github.com/simulot/immich-go/releases/download"
MAX_BINARY_BYTES = 128 * 1024 * 1024


class FetchError(RuntimeError):
    """The pinned release could not be cached safely."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1_048_576):
            digest.update(chunk)
    return digest.hexdigest()


def load_baseline(path: Path) -> dict[str, str]:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
        oracle = value["oracle"]
        artifact = value["artifacts"]["linux_x86_64"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise FetchError(f"invalid oracle baseline: {error}") from error
    selected = {
        "version": oracle.get("version"),
        "archive": artifact.get("archive"),
        "archive_sha256": artifact.get("archive_sha256"),
        "binary_sha256": artifact.get("binary_sha256"),
    }
    if not all(isinstance(item, str) and item for item in selected.values()):
        raise FetchError("oracle baseline is missing artifact fields")
    return selected


def _download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            prefix="oracle-", suffix=".partial", dir=destination.parent, delete=False
        ) as output:
            temporary = Path(output.name)
            with urllib.request.urlopen(url, timeout=60) as response:
                shutil.copyfileobj(response, output, length=1_048_576)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(destination)
    except (OSError, urllib.error.URLError) as error:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
        raise FetchError(f"cannot download pinned oracle: {error}") from error


def _extract_binary(archive: Path, destination: Path, expected_digest: str) -> None:
    temporary: Path | None = None
    try:
        with tarfile.open(archive, "r:gz") as bundle:
            members = bundle.getmembers()
            if {member.name for member in members} != {"LICENSE", "immich-go"}:
                raise FetchError("oracle archive contains an unexpected member set")
            binary_member = bundle.getmember("immich-go")
            if not binary_member.isfile() or not 1 <= binary_member.size <= MAX_BINARY_BYTES:
                raise FetchError("oracle archive binary is not a bounded regular file")
            source = bundle.extractfile(binary_member)
            if source is None:
                raise FetchError("oracle archive binary cannot be read")
            with tempfile.NamedTemporaryFile(
                prefix="immich-go-", suffix=".partial", dir=destination.parent, delete=False
            ) as output:
                temporary = Path(output.name)
                shutil.copyfileobj(source, output, length=1_048_576)
                output.flush()
                os.fsync(output.fileno())
        if _sha256(temporary) != expected_digest:
            raise FetchError("extracted oracle digest mismatch")
        temporary.chmod(0o750)
        temporary.replace(destination)
    except (OSError, tarfile.TarError) as error:
        raise FetchError(f"cannot extract pinned oracle: {error}") from error
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def prepare(cache_root: Path, baseline_path: Path, release_base: str) -> Path:
    baseline = load_baseline(baseline_path)
    cache = cache_root / f"immich-go-{baseline['version']}-linux-x86_64"
    archive = cache / baseline["archive"]
    binary = cache / "immich-go"
    if not archive.exists():
        url = f"{release_base.rstrip('/')}/v{baseline['version']}/{baseline['archive']}"
        _download(url, archive)
    if _sha256(archive) != baseline["archive_sha256"]:
        raise FetchError("cached oracle archive digest mismatch")
    if not binary.exists():
        _extract_binary(archive, binary, baseline["binary_sha256"])
    if not binary.is_file() or _sha256(binary) != baseline["binary_sha256"]:
        raise FetchError("cached oracle binary digest mismatch")
    return binary.resolve()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=REPOSITORY_ROOT / ".cache" / "oracle")
    parser.add_argument("--baseline", type=Path, default=BASELINE_PATH)
    parser.add_argument("--release-base", default=OFFICIAL_RELEASE_BASE)
    arguments = parser.parse_args()
    try:
        binary = prepare(arguments.cache.resolve(), arguments.baseline.resolve(), arguments.release_base)
    except (FetchError, OSError, UnicodeError) as error:
        print(f"oracle fetch failed: {error}", file=sys.stderr)
        return 1
    print(binary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
