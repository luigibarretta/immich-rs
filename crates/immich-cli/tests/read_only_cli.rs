#![forbid(unsafe_code)]

mod support;

use std::fs;
use std::fs::OpenOptions;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::NormalizedPlan;

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const USAGE_EXIT_CODE: i32 = 2;

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("immich-rs-cli-{}-{sequence}", std::process::id()));
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

fn supports_distinct_case_paths(root: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let lower = root.join(".immich-rs-case-probe");
    let upper = root.join(".IMMICH-RS-CASE-PROBE");
    fs::write(&lower, b"probe")?;
    let result = OpenOptions::new().write(true).create_new(true).open(&upper);
    let supported = match result {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error.into()),
    };
    fs::remove_file(&lower)?;
    if supported {
        fs::remove_file(&upper)?;
    }
    Ok(supported)
}

#[test]
fn plan_folder_emits_only_a_valid_normalized_plan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    fs::write(directory.0.join("pixel.png"), b"synthetic-pixel")?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "folder", "--label", "synthetic-cli"])
        .arg(&directory.0)
        .output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let plan: NormalizedPlan = serde_json::from_slice(&output.stdout)?;
    plan.validate()?;
    assert_eq!(plan.source.label, "synthetic-cli");
    assert_eq!(plan.summary.assets, 1);
    Ok(())
}

#[test]
fn mutation_commands_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    for command in ["upload", "delete", "replace", "metadata"] {
        let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
            .arg(command)
            .output()?;
        assert_eq!(output.status.code(), Some(USAGE_EXIT_CODE));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn synthetic_folder_fixture_is_exact_or_fails_closed_on_host_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let distinct_case_paths = supports_distinct_case_paths(&directory.0)?;
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("cannot resolve repository root")?;
    let fixture = repository.join("tests/fixtures/v1/synthetic-folder-matrix");
    let materialized = directory.0.join("source");
    let materialization_output = support::python()
        .arg(repository.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(&materialized)
        .output()?;
    if !distinct_case_paths {
        assert_eq!(materialization_output.status.code(), Some(USAGE_EXIT_CODE));
        assert!(materialization_output.stdout.is_empty());
        assert!(!materialization_output.stderr.is_empty());
        return Ok(());
    }
    assert!(materialization_output.status.success());
    assert!(materialization_output.stdout.is_empty());
    assert!(materialization_output.stderr.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "folder", "--label", "synthetic-folder-matrix"])
        .arg(materialized)
        .output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, fs::read(fixture.join("expected-plan.json"))?);
    Ok(())
}
