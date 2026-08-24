#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;

use immich_rs_core::UploadPlan;

const USAGE_EXIT_CODE: i32 = 2;
const AUTHENTICATION_EXIT_CODE: i32 = 5;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_immich-rs"));
    command
        .env_remove("IMMICH_RS_API_KEY")
        .env_remove("IMMICH_RS_API_KEY_FILE");
    command
}

#[test]
fn remote_upload_planning_requires_cli_acknowledgement_before_secret_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let rejected = command()
        .args([
            "plan",
            "upload",
            "folder",
            "--server",
            "https://example.invalid",
            ".",
        ])
        .output()?;
    assert_eq!(rejected.status.code(), Some(USAGE_EXIT_CODE));
    assert!(rejected.stdout.is_empty());

    let acknowledged = command()
        .args([
            "plan",
            "upload",
            "folder",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            ".",
        ])
        .output()?;
    assert_eq!(acknowledged.status.code(), Some(AUTHENTICATION_EXIT_CODE));
    assert!(acknowledged.stdout.is_empty());
    Ok(())
}

#[test]
fn remote_takeout_planning_requires_cli_acknowledgement_before_secret_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let rejected = command()
        .args([
            "plan",
            "upload",
            "google-takeout",
            "--server",
            "https://example.invalid",
            ".",
        ])
        .output()?;
    assert_eq!(rejected.status.code(), Some(USAGE_EXIT_CODE));
    assert!(rejected.stdout.is_empty());

    let acknowledged = command()
        .args([
            "plan",
            "upload",
            "google-takeout",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            ".",
        ])
        .output()?;
    assert_eq!(acknowledged.status.code(), Some(AUTHENTICATION_EXIT_CODE));
    assert!(acknowledged.stdout.is_empty());
    Ok(())
}

#[test]
fn remote_apple_planning_requires_cli_acknowledgement_before_secret_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let rejected = command()
        .args([
            "plan",
            "upload",
            "apple-photos",
            "--server",
            "https://example.invalid",
            ".",
        ])
        .output()?;
    assert_eq!(rejected.status.code(), Some(USAGE_EXIT_CODE));
    assert!(rejected.stdout.is_empty());

    let acknowledged = command()
        .args([
            "plan",
            "upload",
            "apple-photos",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            ".",
        ])
        .output()?;
    assert_eq!(acknowledged.status.code(), Some(AUTHENTICATION_EXIT_CODE));
    assert!(acknowledged.stdout.is_empty());
    Ok(())
}

#[test]
fn remote_picasa_planning_requires_cli_acknowledgement_before_secret_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let rejected = command()
        .args([
            "plan",
            "upload",
            "picasa",
            "--server",
            "https://example.invalid",
            ".",
        ])
        .output()?;
    assert_eq!(rejected.status.code(), Some(USAGE_EXIT_CODE));
    assert!(rejected.stdout.is_empty());

    let acknowledged = command()
        .args([
            "plan",
            "upload",
            "picasa",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            ".",
        ])
        .output()?;
    assert_eq!(acknowledged.status.code(), Some(AUTHENTICATION_EXIT_CODE));
    assert!(acknowledged.stdout.is_empty());
    Ok(())
}

#[test]
fn remote_archive_planning_requires_the_same_cli_acknowledgement()
-> Result<(), Box<dyn std::error::Error>> {
    let rejected = command()
        .args([
            "plan",
            "archive",
            "immich",
            "--server",
            "https://example.invalid",
        ])
        .output()?;
    assert_eq!(rejected.status.code(), Some(USAGE_EXIT_CODE));

    let acknowledged = command()
        .args([
            "plan",
            "archive",
            "immich",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
        ])
        .output()?;
    assert_eq!(acknowledged.status.code(), Some(AUTHENTICATION_EXIT_CODE));
    Ok(())
}

#[test]
fn upload_plan_inspection_emits_a_stable_confirmation_digest()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "immich-rs-production-plan-{}.json",
        std::process::id()
    ));
    let plan: UploadPlan = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "normalized_plan_sha256": "a".repeat(64),
        "source": {
            "kind": "folder",
            "label": "synthetic-source",
            "fingerprint_sha256": "b".repeat(64),
            "case_sensitive": true,
            "unicode_normalization": "nfc"
        },
        "configuration_sha256": "c".repeat(64),
        "server": {
            "version": {"major": 3, "minor": 1, "patch": 0},
            "identity_sha256": "d".repeat(64)
        },
        "operations": [{
            "operation_id": "e".repeat(64),
            "relative_path": "synthetic.jpg",
            "media_kind": "image",
            "byte_len": 16,
            "content_sha256": "f".repeat(64),
            "created_at_unix_ms": 1,
            "modified_at_unix_ms": 1,
            "xmp_sidecar": null,
            "role": {"kind": "standalone"}
        }],
        "summary": {
            "operations": 1,
            "media_bytes": 16,
            "xmp_sidecars": 0,
            "live_photo_pairs": 0
        }
    }))?;
    fs::write(&path, serde_json::to_vec_pretty(&plan)?)?;
    let first = command()
        .args(["inspect", "upload-plan", "--plan"])
        .arg(&path)
        .output()?;
    let second = command()
        .args(["inspect", "upload-plan", "--plan"])
        .arg(&path)
        .output()?;
    let _cleanup_result = fs::remove_file(&path);

    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let inspection: serde_json::Value = serde_json::from_slice(&first.stdout)?;
    let digest = inspection["plan_sha256"]
        .as_str()
        .ok_or("inspection digest is absent")?;
    assert_eq!(digest.len(), 64);
    assert_eq!(inspection["operations"], 1);
    Ok(())
}

#[test]
fn invalid_custom_ca_fails_before_authentication_or_network()
-> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("immich-rs-invalid-ca-{}.pem", std::process::id()));
    fs::write(&path, b"synthetic invalid CA\n")?;
    let output = command()
        .args([
            "plan",
            "archive",
            "immich",
            "--server",
            "https://example.invalid",
            "--authorize-production-read",
            "--ca-certificate",
        ])
        .arg(&path)
        .output()?;
    let _cleanup_result = fs::remove_file(&path);
    assert_eq!(output.status.code(), Some(USAGE_EXIT_CODE));
    assert!(output.stdout.is_empty());
    Ok(())
}
