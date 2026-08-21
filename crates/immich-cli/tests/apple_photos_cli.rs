#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NORMALIZED_PLAN_SCHEMA_VERSION_V3, NormalizedPlan, SourceKind};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-apple-cli-{}-{sequence}",
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

fn materialize(
    fixture: &std::path::Path,
    output: &std::path::Path,
    archive_view: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository_root()?;
    let mut command = Command::new("python3");
    command
        .arg(repository.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(output);
    if archive_view {
        command.args(["--archive-view", "icloud-split"]);
    }
    let result = command.output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

fn assert_golden(
    output: &[u8],
    fixture: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(output, fs::read(fixture.join("expected-plan.json"))?);
    let plan: NormalizedPlan = serde_json::from_slice(output)?;
    assert_eq!(plan.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V3);
    assert_eq!(plan.source.kind, SourceKind::ApplePhotos);
    plan.validate()?;
    Ok(())
}

#[test]
fn apple_directory_matches_the_versioned_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let fixture = repository_root()?.join("tests/fixtures/v3/synthetic-apple-photos");
    let source = directory.0.join("source");
    materialize(&fixture, &source, false)?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "apple-photos", "--label", "synthetic-apple"])
        .arg(source)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_golden(&output.stdout, &fixture)
}

#[test]
fn apple_split_zip_order_matches_the_same_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let fixture = repository_root()?.join("tests/fixtures/v3/synthetic-apple-photos");
    let source = directory.0.join("archives");
    materialize(&fixture, &source, true)?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "apple-photos", "--label", "synthetic-apple"])
        .arg(source.join("icloud-002.zip"))
        .arg(source.join("icloud-001.zip"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_golden(&output.stdout, &fixture)
}

#[test]
fn apple_album_mode_is_explicit_and_invalid_values_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    fs::create_dir_all(directory.0.join("Synthetic Album"))?;
    fs::write(directory.0.join("Synthetic Album/image.png"), b"synthetic")?;
    let accepted = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "apple-photos", "--album-mode", "folder"])
        .arg(&directory.0)
        .output()?;
    assert!(accepted.status.success());
    let plan: NormalizedPlan = serde_json::from_slice(&accepted.stdout)?;
    assert_eq!(
        plan.assets[0]
            .normalized_metadata
            .as_ref()
            .map(|metadata| &metadata.albums),
        Some(&vec!["Synthetic Album".to_owned()])
    );
    let rejected = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "apple-photos", "--album-mode", "guess"])
        .arg(&directory.0)
        .output()?;
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
    Ok(())
}
