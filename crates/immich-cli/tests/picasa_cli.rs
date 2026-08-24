#![forbid(unsafe_code)]

mod support;

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NORMALIZED_PLAN_SCHEMA_VERSION_V4, NormalizedPlan, SourceKind};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-picasa-cli-{}-{sequence}",
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
        let _cleanup = fs::remove_dir_all(&self.0);
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
    archive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = support::python();
    command
        .arg(repository_root()?.join("scripts/materialize-fixture.py"))
        .arg(fixture.join("manifest.json"))
        .arg(output);
    if archive {
        command.args(["--archive-view", "picasa-split"]);
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
    assert_eq!(plan.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V4);
    assert_eq!(plan.source.kind, SourceKind::Picasa);
    plan.validate()?;
    Ok(())
}

#[test]
fn picasa_directory_matches_the_versioned_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let fixture = repository_root()?.join("tests/fixtures/v4/synthetic-picasa");
    let source = directory.0.join("source");
    materialize(&fixture, &source, false)?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "picasa", "--label", "synthetic-picasa"])
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
fn picasa_split_zip_order_matches_the_same_golden() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let fixture = repository_root()?.join("tests/fixtures/v4/synthetic-picasa");
    let source = directory.0.join("archives");
    materialize(&fixture, &source, true)?;
    let output = Command::new(env!("CARGO_BIN_EXE_immich-rs"))
        .args(["plan", "picasa", "--label", "synthetic-picasa"])
        .arg(source.join("picasa-002.zip"))
        .arg(source.join("picasa-001.zip"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_golden(&output.stdout, &fixture)
}
