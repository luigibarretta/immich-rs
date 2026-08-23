#![forbid(unsafe_code)]

use std::process::Command;

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
