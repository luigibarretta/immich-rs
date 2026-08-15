#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NORMALIZED_PLAN_SCHEMA_VERSION_V2, NeverCancel, NormalizedPlan, SourceKind};
use immich_rs_sources::{FolderScanConfig, NoProgress, scan_google_takeout};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const SOURCE_EXIT_CODE: i32 = 4;

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-takeout-cli-{}-{sequence}",
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

fn repository_root() -> Result<&'static std::path::Path, Box<dyn std::error::Error>> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| "cannot resolve repository root".into())
}

#[test]
fn legacy_takeout_scanner_preserves_the_v1_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let repository = repository_root()?;
    let fixture = repository.join("tests/fixtures/v1/synthetic-google-takeout-basic");
    let source = directory.0.join("source");
    let materialized = Command::new("python3")
        .arg(repository.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(&source)
        .output()?;
    assert!(materialized.status.success());
    assert!(materialized.stdout.is_empty());
    assert!(materialized.stderr.is_empty());

    let plan = scan_google_takeout(
        &source,
        "synthetic-google-takeout-basic",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    let mut actual = serde_json::to_vec_pretty(&plan)?;
    actual.push(b'\n');
    assert_eq!(actual, fs::read(fixture.join("expected-plan.json"))?);
    assert_eq!(plan.source.kind, SourceKind::GoogleTakeout);
    plan.validate()?;
    Ok(())
}

#[test]
fn synthetic_takeout_directory_matches_the_v2_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let repository = repository_root()?;
    let fixture = repository.join("tests/fixtures/v2/synthetic-google-takeout-complete");
    let source = directory.0.join("source");
    let materialized = Command::new("python3")
        .arg(repository.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(&source)
        .output()?;
    assert!(materialized.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args([
            "plan",
            "google-takeout",
            "--label",
            "synthetic-google-takeout-complete",
        ])
        .arg(source)
        .output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, fs::read(fixture.join("expected-plan.json"))?);
    let plan: NormalizedPlan = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V2);
    plan.validate()?;
    Ok(())
}

#[test]
fn takeout_plan_rejects_unknown_layout_without_network_access()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "google-takeout"])
        .arg(&directory.0)
        .env("IMMICH_RS_API_KEY", "synthetic-ignored-key")
        .output()?;
    assert_eq!(output.status.code(), Some(SOURCE_EXIT_CODE));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8(output.stderr)?.contains("synthetic-ignored-key"));
    Ok(())
}

#[test]
fn synthetic_takeout_zip_is_planned_without_extraction() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let repository = repository_root()?;
    let fixture = repository.join("tests/fixtures/v2/synthetic-google-takeout-complete");
    let archives = directory.0.join("archives");
    let materialized = Command::new("python3")
        .arg(repository.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(&archives)
        .args(["--archive-view", "split"])
        .output()?;
    assert!(materialized.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args([
            "plan",
            "google-takeout",
            "--label",
            "synthetic-google-takeout-complete",
        ])
        .arg(archives.join("takeout-002.zip"))
        .arg(archives.join("takeout-001.zip"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, fs::read(fixture.join("expected-plan.json"))?);
    let plan: NormalizedPlan = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V2);
    assert_eq!(plan.summary.assets, 3);
    assert_eq!(plan.summary.sidecars, 5);
    assert!(plan.errors.is_empty());
    plan.validate()?;
    Ok(())
}
