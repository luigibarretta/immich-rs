#![forbid(unsafe_code)]

use std::process::Command;

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_immich-rs-web"));
    command.env_remove("IMMICH_RS_WEB_CONFIG");
    command
}

#[test]
fn help_and_version_are_stable_and_non_secret() -> Result<(), Box<dyn std::error::Error>> {
    let help = binary().arg("--help").output()?;
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("Usage: immich-rs-web [--config PATH]"));
    assert!(help.contains("IMMICH_RS_WEB_CONFIG"));

    let version = binary().arg("--version").output()?;
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout)?,
        format!("immich-rs-web {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(version.stderr.is_empty());
    Ok(())
}

#[test]
fn missing_or_unknown_configuration_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let missing = binary().output()?;
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8(missing.stderr)?.contains("configuration selector is required"));

    let unknown = binary()
        .env("IMMICH_RS_WEB_UNSUPPORTED", "secret-value-must-not-appear")
        .output()?;
    assert!(!unknown.status.success());
    let stderr = String::from_utf8(unknown.stderr)?;
    assert!(stderr.contains("unknown IMMICH_RS_WEB_*"));
    assert!(!stderr.contains("secret-value-must-not-appear"));
    Ok(())
}

#[test]
fn explicit_config_has_precedence_without_echoing_its_path()
-> Result<(), Box<dyn std::error::Error>> {
    let sensitive_path = "/missing/private/operator-web-config.toml";
    let output = binary()
        .env("IMMICH_RS_WEB_CONFIG", "/also/missing.toml")
        .args(["--config", sensitive_path])
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot read web configuration"));
    assert!(!stderr.contains(sensitive_path));
    assert!(!stderr.contains("/also/missing.toml"));
    Ok(())
}
