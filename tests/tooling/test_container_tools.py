from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parents[2]


def load_script(name: str) -> ModuleType:
    path = ROOT / "scripts" / name
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {name}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CONTAINER = load_script("check-container.py")
OCI = load_script("verify-oci-layout.py")


def canonical(value: object) -> bytes:
    return json.dumps(value, separators=(",", ":"), sort_keys=True).encode()


def digest(value: bytes) -> str:
    return f"sha256:{hashlib.sha256(value).hexdigest()}"


def oci_archive(
    path: Path,
    platforms: list[tuple[str, str]],
    revision: str = "a" * 40,
    corrupt_config: bool = False,
    attested_revision: str | None = None,
) -> None:
    blobs: dict[str, bytes] = {}
    descriptors = []
    platform_digests = []
    for os_name, architecture in platforms:
        config = canonical(
            {
                "architecture": architecture,
                "os": os_name,
                "config": {
                    "User": "65532:65532",
                    "Labels": {
                        "org.opencontainers.image.version": "0.0.0",
                        "org.opencontainers.image.revision": revision,
                    },
                },
            }
        )
        config_digest = digest(config)
        blobs[config_digest] = config[:-1] + b"]" if corrupt_config else config
        manifest = canonical(
            {
                "schemaVersion": 2,
                "config": {
                    "mediaType": "application/vnd.oci.image.config.v1+json",
                    "digest": config_digest,
                    "size": len(config),
                },
                "layers": [],
            }
        )
        manifest_digest = digest(manifest)
        blobs[manifest_digest] = manifest
        descriptors.append(
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": manifest_digest,
                "size": len(manifest),
                "platform": {"os": os_name, "architecture": architecture},
            }
        )
        platform_digests.append(manifest_digest)
    for platform_digest in platform_digests:
        layers = []
        for predicate in ("https://spdx.dev/Document", "https://slsa.dev/provenance/v1"):
            statement = canonical(
                {
                    "predicateType": predicate,
                    "predicate": {
                        "spdxVersion": "SPDX-2.3",
                        "dataLicense": "CC0-1.0",
                    },
                }
            )
            if predicate.startswith("https://slsa.dev/provenance/"):
                provenance_revision = attested_revision or revision
                statement = canonical(
                    {
                        "predicateType": predicate,
                        "predicate": {
                            "buildDefinition": {
                                "externalParameters": {
                                    "request": {
                                        "args": {
                                            "build-arg:VCS_REF": provenance_revision
                                        },
                                        "root": {
                                            "request": {
                                                "args": {
                                                    "vcs:revision": provenance_revision
                                                }
                                            }
                                        },
                                    }
                                }
                            },
                            "runDetails": {
                                "metadata": {
                                    "buildkit_metadata": {
                                        "vcs.revision": provenance_revision
                                    }
                                }
                            },
                        },
                    }
                )
            statement_digest = digest(statement)
            blobs[statement_digest] = statement
            layers.append(
                {
                    "digest": statement_digest,
                    "size": len(statement),
                    "annotations": {"in-toto.io/predicate-type": predicate},
                }
            )
        attestation = canonical({"schemaVersion": 2, "layers": layers})
        attestation_digest = digest(attestation)
        blobs[attestation_digest] = attestation
        descriptors.append(
            {
                "digest": attestation_digest,
                "size": len(attestation),
                "platform": {"os": "unknown", "architecture": "unknown"},
                "annotations": {
                    "vnd.docker.reference.type": "attestation-manifest",
                    "vnd.docker.reference.digest": platform_digest,
                },
            }
        )
    nested_index = canonical({"schemaVersion": 2, "manifests": descriptors})
    nested_digest = digest(nested_index)
    blobs[nested_digest] = nested_index
    outer_descriptor = {
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "digest": nested_digest,
        "size": len(nested_index),
    }
    members = {
        "index.json": canonical(
            {"schemaVersion": 2, "manifests": [outer_descriptor]}
        )
    }
    members.update(
        {f"blobs/sha256/{name.removeprefix('sha256:')}": value for name, value in blobs.items()}
    )
    with tarfile.open(path, mode="w") as archive:
        for name, value in sorted(members.items()):
            member = tarfile.TarInfo(name)
            member.size = len(value)
            member.mtime = 0
            archive.addfile(member, io.BytesIO(value))


class ContainerToolTests(unittest.TestCase):
    def test_repository_container_contract_passes(self) -> None:
        self.assertEqual(CONTAINER.check(ROOT), [])

    def test_oci_verifier_requires_both_non_root_platforms(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "image.tar"
            oci_archive(archive, [("linux", "amd64"), ("linux", "arm64")])
            report = OCI.verify(archive, "a" * 40, "0.0.0")
            self.assertEqual(report["schema"], "immich-rs-container-build-v1")
            self.assertEqual(len(report["platforms"]), 2)
            oci_archive(archive, [("linux", "amd64")])
            with self.assertRaisesRegex(OCI.VerificationError, "both required"):
                OCI.verify(archive, "a" * 40, "0.0.0")
            oci_archive(
                archive,
                [("linux", "amd64"), ("linux", "arm64")],
                corrupt_config=True,
            )
            with self.assertRaisesRegex(OCI.VerificationError, "digest drift"):
                OCI.verify(archive, "a" * 40, "0.0.0")
            oci_archive(
                archive,
                [("linux", "amd64"), ("linux", "arm64")],
                attested_revision="b" * 40,
            )
            with self.assertRaisesRegex(OCI.VerificationError, "provenance revision"):
                OCI.verify(archive, "a" * 40, "0.0.0")


if __name__ == "__main__":
    unittest.main()
