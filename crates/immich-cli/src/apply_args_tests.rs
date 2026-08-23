use std::ffi::OsString;

use crate::apply_args::{parse_archive, parse_upload};
use crate::config::EffectiveConfig;

fn arguments(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn production_read_is_limited_to_archive_apply() {
    let config = EffectiveConfig::default();
    let archive = parse_archive(
        &arguments(&[
            "--manifest",
            "manifest.json",
            "--destination",
            "archive",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
        ]),
        &config,
    );
    assert!(matches!(archive, Ok(request) if request.production_read));

    let upload = parse_upload(
        &arguments(&[
            "--plan",
            "plan.json",
            "--source",
            "source",
            "--checkpoint",
            "checkpoint.sqlite",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
        ]),
        &config,
    );
    assert!(upload.is_err());
}

#[test]
fn production_upload_requires_the_complete_cli_confirmation() {
    let config = EffectiveConfig::default();
    let complete = parse_upload(
        &arguments(&[
            "--plan",
            "plan.json",
            "--source",
            "source",
            "--checkpoint",
            "checkpoint.sqlite",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            "--authorize-production-write",
            "--confirm-plan-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--expected-operations",
            "1",
            "--backup-reference",
            "synthetic-backup",
        ]),
        &config,
    );
    assert!(matches!(complete, Ok(request) if request.production.is_some()));

    let dry_run = parse_upload(
        &arguments(&[
            "--plan",
            "plan.json",
            "--source",
            "source",
            "--checkpoint",
            "checkpoint.sqlite",
            "--dry-run",
            "--authorize-production-write",
        ]),
        &config,
    );
    assert!(matches!(dry_run, Ok(request) if request.production.is_none()));
}

#[test]
fn repeated_inputs_are_ordered_and_cannot_mix_with_source() {
    let parsed = parse_upload(
        &arguments(&[
            "--plan",
            "plan.json",
            "--checkpoint",
            "checkpoint.sqlite",
            "--dry-run",
            "--input",
            "one.zip",
            "--input",
            "two.zip",
        ]),
        &EffectiveConfig::default(),
    );
    assert!(matches!(parsed, Ok(request) if request.inputs.len() == 2));

    let mixed = parse_upload(
        &arguments(&[
            "--plan",
            "plan.json",
            "--checkpoint",
            "checkpoint.sqlite",
            "--dry-run",
            "--source",
            "source",
            "--input",
            "one.zip",
        ]),
        &EffectiveConfig::default(),
    );
    assert!(mixed.is_err());
}
