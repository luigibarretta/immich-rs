#!/usr/bin/env python3
"""Verify and summarize one bounded amd64/arm64 OCI image archive."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tarfile
from pathlib import Path
from typing import Any

EXPECTED_PLATFORMS = {("linux", "amd64"), ("linux", "arm64")}
MAX_INDEX_BYTES = 1024 * 1024
MAX_ATTESTATION_BYTES = 4 * 1024 * 1024
SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[1-9][0-9]*)?$")


class VerificationError(ValueError):
    """One OCI archive contract violation."""


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--image-title", choices=("immich-rs", "immich-rs-web"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def bounded_json(
    archive: tarfile.TarFile,
    name: str,
    expected_size: int | None = None,
    max_bytes: int = MAX_INDEX_BYTES,
) -> dict[str, Any]:
    try:
        member = archive.getmember(name)
    except KeyError as error:
        raise VerificationError(f"missing OCI member: {name}") from error
    if not member.isfile() or not 0 < member.size <= max_bytes:
        raise VerificationError(f"invalid bounded OCI member: {name}")
    if expected_size is not None and member.size != expected_size:
        raise VerificationError(f"OCI descriptor size drift: {name}")
    stream = archive.extractfile(member)
    if stream is None:
        raise VerificationError(f"cannot read OCI member: {name}")
    try:
        content = stream.read(member.size + 1)
        if len(content) != member.size:
            raise VerificationError(f"OCI member size drift: {name}")
        if name.startswith("blobs/sha256/"):
            expected = name.removeprefix("blobs/sha256/")
            if hashlib.sha256(content).hexdigest() != expected:
                raise VerificationError(f"OCI member digest drift: {name}")
        value = json.loads(content)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"invalid OCI JSON member: {name}") from error
    if not isinstance(value, dict):
        raise VerificationError(f"OCI member is not an object: {name}")
    return value


def descriptor_json(
    archive: tarfile.TarFile,
    descriptor: dict[str, Any],
    max_bytes: int = MAX_INDEX_BYTES,
) -> dict[str, Any]:
    digest = descriptor.get("digest")
    size = descriptor.get("size")
    if not isinstance(digest, str) or not isinstance(size, int):
        raise VerificationError("OCI JSON descriptor is incomplete")
    return bounded_json(archive, blob_name(digest), size, max_bytes)


def manifest_descriptors(
    archive: tarfile.TarFile, index: dict[str, Any]
) -> list[Any]:
    descriptors = index.get("manifests")
    if index.get("schemaVersion") != 2 or not isinstance(descriptors, list):
        raise VerificationError("OCI index schema or manifests are invalid")
    if (
        len(descriptors) == 1
        and isinstance(descriptors[0], dict)
        and "platform" not in descriptors[0]
        and descriptors[0].get("mediaType")
        == "application/vnd.oci.image.index.v1+json"
    ):
        nested = descriptor_json(archive, descriptors[0])
        descriptors = nested.get("manifests")
        if nested.get("schemaVersion") != 2 or not isinstance(descriptors, list):
            raise VerificationError("nested OCI index is invalid")
    return descriptors


def object_member(value: dict[str, Any], key: str) -> dict[str, Any]:
    member = value.get(key)
    if not isinstance(member, dict):
        raise VerificationError(f"OCI provenance object is missing: {key}")
    return member


def verify_provenance(
    statement: dict[str, Any], predicate_type: str, commit_sha: str
) -> None:
    if statement.get("predicateType") != predicate_type:
        raise VerificationError("OCI provenance predicate annotation drift")
    predicate = object_member(statement, "predicate")
    build_definition = object_member(predicate, "buildDefinition")
    external = object_member(build_definition, "externalParameters")
    request = object_member(external, "request")
    request_args = object_member(request, "args")
    root_request = object_member(object_member(request, "root"), "request")
    root_args = object_member(root_request, "args")
    run_details = object_member(predicate, "runDetails")
    metadata = object_member(run_details, "metadata")
    buildkit_metadata = object_member(metadata, "buildkit_metadata")
    declared_revision = request_args.get("build-arg:VCS_REF")
    source_revision = root_args.get("vcs:revision")
    metadata_revision = buildkit_metadata.get("vcs.revision")
    if (
        declared_revision != commit_sha
        or source_revision != commit_sha
        or metadata_revision not in (None, commit_sha)
    ):
        raise VerificationError("OCI provenance revision does not match the exact commit")


def verify_sbom(statement: dict[str, Any], predicate_type: str) -> None:
    if statement.get("predicateType") != predicate_type:
        raise VerificationError("OCI SBOM predicate annotation drift")
    predicate = object_member(statement, "predicate")
    spdx_version = predicate.get("spdxVersion")
    if not isinstance(spdx_version, str) or not spdx_version.startswith("SPDX-"):
        raise VerificationError("OCI SBOM is not an SPDX document")
    if predicate.get("dataLicense") != "CC0-1.0":
        raise VerificationError("OCI SBOM data license is invalid")


def blob_name(digest: str) -> str:
    if not SHA256.fullmatch(digest):
        raise VerificationError("OCI descriptor has an invalid SHA-256 digest")
    return f"blobs/sha256/{digest.removeprefix('sha256:')}"


def verify(path: Path, commit_sha: str, version: str, image_title: str) -> dict[str, Any]:
    if not re.fullmatch(r"[0-9a-f]{40}", commit_sha):
        raise VerificationError("commit SHA must be exact lowercase SHA-1")
    if VERSION.fullmatch(version) is None or image_title not in ("immich-rs", "immich-rs-web"):
        raise VerificationError("container version is invalid")
    if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
        raise VerificationError("OCI archive must be a non-empty regular file")
    archive_hash = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            archive_hash.update(chunk)
    with tarfile.open(path, mode="r") as archive:
        index = bounded_json(archive, "index.json")
        descriptors = manifest_descriptors(archive, index)
        platforms: dict[tuple[str, str], str] = {}
        attestation_descriptors: list[dict[str, Any]] = []
        for descriptor in descriptors:
            if not isinstance(descriptor, dict) or not isinstance(
                descriptor.get("platform"), dict
            ):
                raise VerificationError("OCI manifest descriptor is invalid")
            platform = descriptor["platform"]
            pair = (platform.get("os"), platform.get("architecture"))
            if pair not in EXPECTED_PLATFORMS:
                attestation_descriptors.append(descriptor)
                continue
            digest = descriptor.get("digest")
            if not isinstance(digest, str) or pair in platforms:
                raise VerificationError("OCI platform descriptor is duplicate or incomplete")
            manifest = descriptor_json(archive, descriptor)
            if manifest.get("schemaVersion") != 2 or not isinstance(
                manifest.get("config"), dict
            ):
                raise VerificationError("OCI image manifest is invalid")
            config_descriptor = manifest["config"]
            config_digest = config_descriptor.get("digest")
            config_size = config_descriptor.get("size")
            if not isinstance(config_digest, str) or not isinstance(config_size, int):
                raise VerificationError("OCI image config descriptor is invalid")
            image_config = bounded_json(
                archive,
                blob_name(config_digest),
                config_size,
            )
            if (image_config.get("os"), image_config.get("architecture")) != pair:
                raise VerificationError("OCI config platform does not match its descriptor")
            runtime_config = image_config.get("config")
            if not isinstance(runtime_config, dict):
                raise VerificationError("OCI runtime config is invalid")
            if runtime_config.get("User") != "65532:65532":
                raise VerificationError("OCI platform does not run as UID/GID 65532")
            labels = runtime_config.get("Labels")
            labels_match = (
                isinstance(labels, dict)
                and labels.get("org.opencontainers.image.title") == image_title
                and labels.get("org.opencontainers.image.version") == version
                and labels.get("org.opencontainers.image.revision") == commit_sha
            )
            if not labels_match:
                raise VerificationError("OCI labels do not match the requested version and revision")
            platforms[pair] = digest
    if set(platforms) != EXPECTED_PLATFORMS:
        raise VerificationError("OCI archive does not contain exactly both required Linux platforms")
    with tarfile.open(path, mode="r") as archive:
        attested: set[str] = set()
        for descriptor in attestation_descriptors:
            annotations = descriptor.get("annotations")
            if not isinstance(annotations, dict) or annotations.get(
                "vnd.docker.reference.type"
            ) != "attestation-manifest":
                raise VerificationError("OCI index contains an unexpected platform")
            reference = annotations.get("vnd.docker.reference.digest")
            descriptor_digest = descriptor.get("digest")
            if (
                not isinstance(reference, str)
                or reference not in platforms.values()
                or not isinstance(descriptor_digest, str)
                or reference in attested
            ):
                raise VerificationError("OCI attestation reference is invalid or duplicate")
            manifest = descriptor_json(archive, descriptor)
            layers = manifest.get("layers")
            if not isinstance(layers, list):
                raise VerificationError("OCI attestation manifest has no layers")
            predicate_layers = [
                (
                    layer.get("annotations", {}).get("in-toto.io/predicate-type"),
                    layer,
                )
                for layer in layers
                if isinstance(layer, dict) and isinstance(layer.get("annotations"), dict)
            ]
            sbom_layers = [
                layer
                for predicate_type, layer in predicate_layers
                if isinstance(predicate_type, str)
                and predicate_type.startswith("https://spdx.dev/")
            ]
            provenance_layers = [
                (predicate_type, layer)
                for predicate_type, layer in predicate_layers
                if isinstance(predicate_type, str)
                and predicate_type.startswith("https://slsa.dev/provenance/")
            ]
            if len(sbom_layers) != 1 or len(provenance_layers) != 1:
                raise VerificationError("OCI platform lacks SBOM or provenance attestation")
            sbom_type = sbom_layers[0]["annotations"]["in-toto.io/predicate-type"]
            sbom_statement = descriptor_json(
                archive, sbom_layers[0], MAX_ATTESTATION_BYTES
            )
            verify_sbom(sbom_statement, sbom_type)
            predicate_type, provenance_layer = provenance_layers[0]
            statement = descriptor_json(
                archive, provenance_layer, MAX_ATTESTATION_BYTES
            )
            verify_provenance(statement, predicate_type, commit_sha)
            attested.add(reference)
        if attested != set(platforms.values()):
            raise VerificationError("OCI platforms are not both attested")
    return {
        "schema": "immich-rs-container-build-v1",
        "commit_sha": commit_sha,
        "version": version,
        "image_title": image_title,
        "archive_sha256": archive_hash.hexdigest(),
        "archive_bytes": path.stat().st_size,
        "platforms": [
            {
                "os": os_name,
                "architecture": architecture,
                "manifest_digest": platforms[(os_name, architecture)],
            }
            for os_name, architecture in sorted(EXPECTED_PLATFORMS)
        ],
    }


def main() -> int:
    args = arguments()
    try:
        if args.output.exists() or args.output.is_symlink():
            raise VerificationError("OCI report output already exists or is linked")
        report = verify(args.archive, args.commit_sha, args.version, args.image_title)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (OSError, tarfile.TarError, VerificationError) as error:
        print(f"OCI verification failed: {error}", file=sys.stderr)
        return 1
    print("OCI archive passed: linux/amd64 and linux/arm64, non-root")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
