#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NormalizedPlan, SourceKind};

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
fn synthetic_takeout_fixture_matches_the_versioned_golden() -> Result<(), Box<dyn std::error::Error>>
{
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

    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args([
            "plan",
            "google-takeout",
            "--label",
            "synthetic-google-takeout-basic",
        ])
        .arg(source)
        .output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, fs::read(fixture.join("expected-plan.json"))?);
    let plan: NormalizedPlan = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan.source.kind, SourceKind::GoogleTakeout);
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
