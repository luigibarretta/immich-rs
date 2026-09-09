#!/usr/bin/env python3
"""Enforce workspace dependency and Phase-2 capability boundaries."""

from __future__ import annotations

from pathlib import Path
import sys
import tomllib

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = {
    "immich-rs-application": REPOSITORY_ROOT / "crates" / "immich-application" / "Cargo.toml",
    "immich-rs-cli": REPOSITORY_ROOT / "crates" / "immich-cli" / "Cargo.toml",
    "immich-rs-client": REPOSITORY_ROOT / "crates" / "immich-client" / "Cargo.toml",
    "immich-rs-core": REPOSITORY_ROOT / "crates" / "immich-core" / "Cargo.toml",
    "immich-rs-executor": REPOSITORY_ROOT / "crates" / "immich-executor" / "Cargo.toml",
    "immich-rs-sources": REPOSITORY_ROOT / "crates" / "immich-sources" / "Cargo.toml",
}
ALLOWED_INTERNAL = {
    "immich-rs-application": {"immich-rs-core", "immich-rs-sources"},
    "immich-rs-cli": {
        "immich-rs-application",
        "immich-rs-client",
        "immich-rs-core",
        "immich-rs-executor",
        "immich-rs-sources",
    },
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
    folder_frontend = (
        REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / "folder.rs"
    ).read_text(encoding="utf-8")
    if (
        "immich_rs_application" not in folder_frontend
        or "immich_rs_sources" in folder_frontend
        or "scan_folder(" in folder_frontend
    ):
        failures.append("folder CLI does not use only the application workflow facade")
    application_source = "\n".join(
        path.read_text(encoding="utf-8")
        for path in (REPOSITORY_ROOT / "crates" / "immich-application" / "src").glob("*.rs")
    ).casefold()
    forbidden_application_tokens = (
        "immich_rs_client",
        "immich_rs_executor",
        "std::env",
        "println!",
        "eprintln!",
        "process::exit",
    )
    if any(token in application_source for token in forbidden_application_tokens):
        failures.append("read-only application facade can reach effects or process concerns")
    dry_run_source = (REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / "upload_dry_run.rs").read_text(
        encoding="utf-8"
    )
    forbidden_dry_run_tokens = ("immich_rs_client", "api_key", "server", "authorize_upload")
    if any(token in dry_run_source.casefold() for token in forbidden_dry_run_tokens):
        failures.append("dry-run source can reach a server or upload capability")
    takeout_source = (
        REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / "google_takeout.rs"
    ).read_text(encoding="utf-8")
    forbidden_takeout_tokens = (
        "immich_rs_client",
        "immich_rs_executor",
        "api_key",
        "server",
        "upload",
    )
    if any(token in takeout_source.casefold() for token in forbidden_takeout_tokens):
        failures.append("Google Takeout plan source can reach a mutation capability")
    archive_sources = "\n".join(
        (REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / name).read_text(
            encoding="utf-8"
        )
        for name in ("archive_plan.rs", "archive_apply.rs")
    ).casefold()
    forbidden_archive_tokens = (
        "authorize_upload",
        "immichuploadclient",
        "apply_upload",
        "create_upload_plan",
    )
    if any(token in archive_sources for token in forbidden_archive_tokens):
        failures.append("archive command can construct a server mutation capability")
    client_root = REPOSITORY_ROOT / "crates" / "immich-client" / "src"
    client_source = (client_root / "lib.rs").read_text(encoding="utf-8")
    upload_source = (client_root / "upload.rs").read_text(encoding="utf-8")
    read_source = (client_root / "read.rs").read_text(encoding="utf-8")
    if "ImmichReadClient" not in client_source or "ImmichUploadClient" not in client_source:
        failures.append("client does not expose separate read and upload capability types")
    if "pub fn authorize_upload" not in read_source or "NegotiatedServer" not in read_source:
        failures.append("upload capability is not restricted to an opaque probe proof")
    production_source = (client_root / "production.rs").read_text(encoding="utf-8")
    production_requirements = (
        "ProductionImmichUploadClient",
        "ProductionUploadAuthorization",
        "backup_reference_sha256",
        "matches_plan",
    )
    if (
        "pub fn authorize_production_upload" not in read_source
        or any(token not in production_source for token in production_requirements)
    ):
        failures.append("production upload lacks an exact plan and backup capability binding")
    forbidden_mutations = ("delete(", "replace(", "put(", "patch(")
    if any(token in upload_source.casefold() for token in forbidden_mutations):
        failures.append("Phase-2 client contains an unapproved mutation primitive")
    apply_arguments = (
        REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / "apply_args.rs"
    ).read_text(encoding="utf-8")
    production_flags = (
        "--authorize-production-read",
        "--authorize-production-write",
        "--confirm-plan-sha256",
        "--expected-operations",
        "--backup-reference",
    )
    if any(flag not in apply_arguments for flag in production_flags):
        failures.append("production apply does not require the complete CLI confirmation")
    configuration_sources = "\n".join(
        (REPOSITORY_ROOT / "crates" / "immich-cli" / "src" / name).read_text(
            encoding="utf-8"
        )
        for name in ("config.rs", "environment.rs")
    )
    if any(
        flag.lstrip("-").replace("-", "_") in configuration_sources
        for flag in production_flags
    ):
        failures.append("production authorization can be persisted outside the CLI")
    executor_apply = (
        REPOSITORY_ROOT / "crates" / "immich-executor" / "src" / "apply.rs"
    ).read_text(encoding="utf-8")
    if (
        "if client.is_production()" not in executor_apply
        or "apply_production_upload" not in executor_apply
    ):
        failures.append("production transport can bypass the authorized executor path")
    journal_source = (
        REPOSITORY_ROOT / "crates" / "immich-executor" / "src" / "journal.rs"
    ).read_text(encoding="utf-8")
    if '"backup_reference"' in journal_source or "backup_reference_sha256" not in journal_source:
        failures.append("checkpoint does not hash the production backup reference")
    disposable_sources = "\n".join(
        path.read_text(encoding="utf-8")
        for path in (
            REPOSITORY_ROOT / ".gitea" / "workflows" / "disposable.yml",
            REPOSITORY_ROOT / ".gitea" / "workflows" / "phase2-benchmark.yml",
            REPOSITORY_ROOT / "scripts" / "run-disposable-immich.sh",
            REPOSITORY_ROOT / "scripts" / "benchmark-phase2.py",
            REPOSITORY_ROOT / "scripts" / "materialize-phase2-corpus.sh",
            REPOSITORY_ROOT / "scripts" / "prepare-phase2-benchmark-corpus.sh",
            REPOSITORY_ROOT / "scripts" / "run-disposable-archive.sh",
            REPOSITORY_ROOT / "scripts" / "run-disposable-production.sh",
        )
    ).casefold()
    forbidden_transport = ("--network host", "host.docker.internal", "host-gateway")
    if any(token in disposable_sources for token in forbidden_transport):
        failures.append("disposable gate can reach the Docker host network")
    harness = (REPOSITORY_ROOT / "scripts" / "run-disposable-immich.sh").read_text(
        encoding="utf-8"
    )
    if '--target-container "$SERVER"' not in harness or "--target-port" in harness:
        failures.append("containerized loopback forwarder is not bound to disposable Immich")
    archive_harness = (
        REPOSITORY_ROOT / "scripts" / "run-disposable-archive.sh"
    ).read_text(encoding="utf-8")
    if (
        '--target-container "$SERVER"' not in archive_harness
        or "--network host" in archive_harness
        or "asset.update" in archive_harness
        or "asset.delete" in archive_harness
    ):
        failures.append("archive disposable gate violates its isolation boundary")
    production_harness = (
        REPOSITORY_ROOT / "scripts" / "run-disposable-production.sh"
    ).read_text(encoding="utf-8")
    production_workflow = (
        REPOSITORY_ROOT / ".gitea" / "workflows" / "production-disposable.yml"
    ).read_text(encoding="utf-8")
    push_workflow = (REPOSITORY_ROOT / ".gitea" / "workflows" / "ci.yml").read_text(
        encoding="utf-8"
    )
    if (
        "com.docker.network.bridge.enable_ip_masquerade=false" not in production_harness
        or 'scripts/tls-forward.py" --listen-host "$TLS_HOST"' not in production_harness
        or "workflow_dispatch" not in production_workflow
        or "phase7-disposable-*" not in production_workflow
        or "branches:" in production_workflow
        or "python3 scripts/check-production-evidence.py" not in push_workflow
    ):
        failures.append("production HTTPS gate is not isolated and evidence-enforced")
    materializer = (REPOSITORY_ROOT / "scripts" / "materialize-phase2-corpus.sh").read_text(
        encoding="utf-8"
    )
    if "--network none" not in materializer or "/usr/bin/ffmpeg" not in materializer:
        failures.append("synthetic media generator is not pinned to an offline tool")
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
