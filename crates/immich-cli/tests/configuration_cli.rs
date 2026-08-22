#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::NormalizedPlan;

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const USAGE_EXIT_CODE: i32 = 2;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-config-cli-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_immich-rs"));
    command.env_clear().env("LANG", "C.UTF-8");
    command
}

fn quoted(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn plan(output: &Output) -> Result<NormalizedPlan, Box<dyn std::error::Error>> {
    if !output.status.success() {
        return Err(format!(
            "plan failed: exit={:?}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn precedence_is_cli_then_environment_then_toml() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let source = directory.0.join("source");
    fs::create_dir(&source)?;
    fs::write(
        source.join("synthetic.jpg"),
        b"synthetic configuration fixture\n",
    )?;
    let config = directory.0.join("immich-rs.toml");
    fs::write(
        &config,
        format!(
            "schema_version = 1\n\n[scan]\nlabel = \"toml-label\"\nsource = \"{}\"\nbuffer_bytes = 4096\n",
            quoted(&source)
        ),
    )?;

    let toml_output = command()
        .args(["--config"])
        .arg(&config)
        .args(["plan", "folder"])
        .output()?;
    let toml_plan = plan(&toml_output)?;
    assert_eq!(toml_plan.source.label, "toml-label");

    let environment_output = command()
        .env("IMMICH_RS_CONFIG", &config)
        .env("IMMICH_RS_LABEL", "environment-label")
        .args(["plan", "folder"])
        .output()?;
    let environment_plan = plan(&environment_output)?;
    assert_eq!(environment_plan.source.label, "environment-label");

    let cli_output = command()
        .env("IMMICH_RS_CONFIG", &config)
        .env("IMMICH_RS_LABEL", "environment-label")
        .args(["plan", "folder", "--label", "cli-label"])
        .output()?;
    let cli_plan = plan(&cli_output)?;
    assert_eq!(cli_plan.source.label, "cli-label");
    Ok(())
}

#[test]
fn explicit_config_path_replaces_environment_path() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let first = directory.0.join("first.toml");
    let second = directory.0.join("second.toml");
    fs::write(&first, "schema_version = 1\n[scan]\nlabel = \"first\"\n")?;
    fs::write(&second, "schema_version = 1\n[scan]\nlabel = \"second\"\n")?;
    let output = command()
        .env("IMMICH_RS_CONFIG", &first)
        .args(["--config"])
        .arg(&second)
        .args(["config", "show"])
        .output()?;
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["label"], "second");
    Ok(())
}

#[test]
fn strict_toml_rejects_unknown_secret_and_schema_keys() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let cases = [
        "schema_version = 1\n[scan]\nmystery = true\n",
        "schema_version = 2\n",
        "schema_version = 1\n[immich]\napi_key = \"synthetic\"\n",
    ];
    for (index, contents) in cases.iter().enumerate() {
        let path = directory.0.join(format!("invalid-{index}.toml"));
        fs::write(&path, contents)?;
        let output = command()
            .args(["--config"])
            .arg(path)
            .args(["config", "show"])
            .output()?;
        assert_eq!(output.status.code(), Some(USAGE_EXIT_CODE));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn unknown_environment_names_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let output = command()
        .env("IMMICH_RS_LBAEL", "synthetic-misspelling")
        .args(["config", "show"])
        .output()?;
    assert_eq!(output.status.code(), Some(USAGE_EXIT_CODE));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("IMMICH_RS_LBAEL"));
    assert!(!stderr.contains("synthetic-misspelling"));
    Ok(())
}

#[test]
fn effective_configuration_never_renders_api_key() -> Result<(), Box<dyn std::error::Error>> {
    let output = command()
        .env("IMMICH_RS_API_KEY", "synthetic-secret-value")
        .env("IMMICH_RS_SERVER", "http://127.0.0.1:2283")
        .env("IMMICH_RS_CONCURRENCY", "2")
        .env("IMMICH_RS_ARCHIVE_INCLUDE_TRASHED", "true")
        .args(["config", "show"])
        .output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    assert!(!text.contains("synthetic-secret-value"));
    assert!(!text.contains("API_KEY"));
    let value: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["concurrency"], 2);
    assert_eq!(value["archive_include_trashed"], true);
    Ok(())
}
