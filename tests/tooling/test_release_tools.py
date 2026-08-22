"""Tests for fail-closed native release tooling without signing material."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {filename}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


PREFLIGHT = load("release_preflight", "release-preflight.py")
PACKAGE = load("package_release", "package-release.py")
FINALIZE = load("finalize_release", "finalize-release.py")


class ReleaseToolTests(unittest.TestCase):
    def test_candidate_tag_contract(self) -> None:
        self.assertEqual(PREFLIGHT.tag_version("v0.1.0-rc.1"), "0.1.0-rc.1")
        for invalid in ("v0.1.0", "0.1.0-rc.1", "v0.1.0-rc.0", "v0.1-rc.1"):
            with self.subTest(invalid=invalid), self.assertRaises(PREFLIGHT.ReleaseError):
                PREFLIGHT.tag_version(invalid)

    def test_archive_metadata_is_normalized(self) -> None:
        tar = PACKAGE.tar_entry("immich-rs/file", 3, 315_532_800, True)
        zipped = PACKAGE.zip_entry("immich-rs/file", 315_532_800, False)
        self.assertEqual((tar.uid, tar.gid, tar.mode), (0, 0, 0o755))
        self.assertEqual(zipped.external_attr >> 16, 0o644)

    def test_finalizer_requires_all_five_targets(self) -> None:
        version = "0.1.0-rc.1"
        revision = "a" * 40
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for target, extension in FINALIZE.TARGETS.items():
                stem = f"immich-rs-{version}-{target}"
                (root / f"{stem}.{extension}").write_bytes(b"synthetic archive")
                (root / f"{stem}.sbom.cdx.json").write_text(
                    json.dumps({"bomFormat": "CycloneDX"}), encoding="utf-8"
                )
                (root / f"{stem}.provenance.json").write_text(
                    json.dumps(
                        {
                            "schema": "immich-rs-build-provenance-v1",
                            "version": version,
                            "source_revision": revision,
                        }
                    ),
                    encoding="utf-8",
                )
            container_archive = root / f"immich-rs-{version}-linux-multiarch.oci.tar"
            container_archive.write_bytes(b"synthetic multiarch OCI archive")
            (root / f"immich-rs-{version}-linux-multiarch.container.json").write_text(
                json.dumps(
                    {
                        "schema": "immich-rs-container-build-v1",
                        "version": version,
                        "commit_sha": revision,
                        "archive_sha256": FINALIZE.sha256(container_archive),
                        "archive_bytes": container_archive.stat().st_size,
                        "platforms": [
                            {
                                "os": "linux",
                                "architecture": architecture,
                                "manifest_digest": "sha256:" + digest * 64,
                            }
                            for architecture, digest in (("amd64", "1"), ("arm64", "2"))
                        ],
                    }
                ),
                encoding="utf-8",
            )
            output = root / "SHA256SUMS"
            FINALIZE.finalize(root, version, revision, output)
            lines = output.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 17)
            self.assertEqual(lines, sorted(lines, key=lambda line: line.split("  ", 1)[1]))

    def test_native_package_is_byte_reproducible(self) -> None:
        version = "0.1.0-rc.1"
        revision = "b" * 40
        target = "x86_64-unknown-linux-gnu"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "immich-rs"
            binary.write_bytes(b"synthetic executable")
            sbom = root / "sbom.cdx.json"
            sbom.write_text(json.dumps({"bomFormat": "CycloneDX"}), encoding="utf-8")
            outputs = []
            with mock.patch.object(PACKAGE, "workspace_version", return_value=version), mock.patch.object(
                PACKAGE, "host_target", return_value=target
            ):
                for index in range(2):
                    destination = root / f"dist-{index}"
                    arguments = SimpleNamespace(
                        target=target,
                        runner="docker",
                        binary=binary,
                        sbom=sbom,
                        revision=revision,
                        tag=f"v{version}",
                        epoch=1_700_000_000,
                        output=destination,
                    )
                    outputs.append(PACKAGE.package(arguments)[0].read_bytes())
            self.assertEqual(outputs[0], outputs[1])


if __name__ == "__main__":
    unittest.main()
