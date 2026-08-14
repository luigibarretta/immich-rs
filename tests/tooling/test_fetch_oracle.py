"""Tests for checksum-pinned oracle caching without network access."""

from __future__ import annotations

from io import BytesIO
import hashlib
import importlib.util
from pathlib import Path
import stat
import tarfile
import tempfile
import textwrap
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_fetcher():
    path = REPOSITORY_ROOT / "scripts" / "fetch-oracle.py"
    spec = importlib.util.spec_from_file_location("tested_oracle_fetcher", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fetcher = load_fetcher()


class OracleFetcherTests(unittest.TestCase):
    def test_prepares_exact_binary_and_reuses_cached_archive(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-fetch-") as temporary:
            root = Path(temporary)
            release = root / "release" / "v0.32.0"
            release.mkdir(parents=True)
            archive = release / "immich-go_Linux_x86_64.tar.gz"
            binary_bytes = b"#!/bin/sh\necho synthetic oracle\n"
            with tarfile.open(archive, "w:gz") as bundle:
                for name, content in (("LICENSE", b"synthetic license\n"), ("immich-go", binary_bytes)):
                    member = tarfile.TarInfo(name)
                    member.size = len(content)
                    member.mode = 0o750 if name == "immich-go" else 0o640
                    bundle.addfile(member, BytesIO(content))
            baseline = root / "baseline.toml"
            baseline.write_text(
                textwrap.dedent(
                    f"""\
                    [oracle]
                    version = "0.32.0"

                    [artifacts.linux_x86_64]
                    archive = "{archive.name}"
                    archive_sha256 = "{hashlib.sha256(archive.read_bytes()).hexdigest()}"
                    binary_sha256 = "{hashlib.sha256(binary_bytes).hexdigest()}"
                    """
                ),
                encoding="utf-8",
            )
            binary = fetcher.prepare(root / "cache", baseline, (root / "release").as_uri())
            self.assertEqual(binary.read_bytes(), binary_bytes)
            self.assertTrue(binary.stat().st_mode & stat.S_IXUSR)
            second = fetcher.prepare(root / "cache", baseline, "https://unreachable.invalid")
            self.assertEqual(second, binary)

    def test_rejects_corrupt_cached_archive(self) -> None:
        with tempfile.TemporaryDirectory(prefix="immich-rs-fetch-corrupt-") as temporary:
            root = Path(temporary)
            baseline = root / "baseline.toml"
            baseline.write_text(
                "[oracle]\nversion='0.32.0'\n[artifacts.linux_x86_64]\n"
                "archive='oracle.tar.gz'\narchive_sha256='" + "0" * 64 + "'\n"
                "binary_sha256='" + "1" * 64 + "'\n",
                encoding="utf-8",
            )
            cache = root / "cache" / "immich-go-0.32.0-linux-x86_64"
            cache.mkdir(parents=True)
            (cache / "oracle.tar.gz").write_bytes(b"corrupt")
            with self.assertRaises(fetcher.FetchError):
                fetcher.prepare(root / "cache", baseline, "https://unreachable.invalid")


if __name__ == "__main__":
    unittest.main()
