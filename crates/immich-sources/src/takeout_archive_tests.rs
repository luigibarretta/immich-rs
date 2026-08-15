use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NORMALIZED_PLAN_SCHEMA_VERSION_V2, NeverCancel, rule_id};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::{NoProgress, ScanError, TakeoutScanConfig, scan_google_takeout_inputs};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-takeout-archive-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn archive(
        &self,
        name: &str,
        entries: &[(&str, &[u8])],
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.0.join(name);
        let file = File::create(&path)?;
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (entry_path, content) in entries {
            writer.start_file(*entry_path, options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn scan(inputs: &[PathBuf]) -> Result<immich_rs_core::NormalizedPlan, ScanError> {
    scan_google_takeout_inputs(
        inputs,
        "synthetic-split",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn split_archives_are_streamed_and_input_order_is_not_semantic()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("split")?;
    let first = directory.archive(
        "takeout-001.zip",
        &[
            (
                "Takeout/Google Photos/Photos from 2024/alpha.png",
                b"synthetic-alpha",
            ),
            (
                "Takeout/Google Photos/Photos from 2024/alpha.png.json",
                br#"{"title":"alpha.png"}"#,
            ),
        ],
    )?;
    let second = directory.archive(
        "takeout-002.zip",
        &[
            (
                "Takeout/Google Photos/Photos from 2024/beta.png",
                b"synthetic-beta",
            ),
            (
                "Takeout/Google Photos/Photos from 2024/beta.png.json",
                br#"{"title":"beta.png"}"#,
            ),
        ],
    )?;
    let forward = scan(&[first.clone(), second.clone()])?;
    let reverse = scan(&[second, first])?;
    assert_eq!(forward, reverse);
    assert_eq!(forward.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V2);
    assert_eq!(forward.summary.assets, 2);
    assert_eq!(forward.summary.sidecars, 2);
    assert!(forward.errors.is_empty());
    Ok(())
}

#[test]
fn identical_and_conflicting_logical_entries_are_explicit() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new("duplicates")?;
    let first = directory.archive(
        "takeout-001.zip",
        &[("Takeout/Google Photos/Photos from 2024/alpha.png", b"same")],
    )?;
    let identical = directory.archive(
        "takeout-002.zip",
        &[("Takeout/Google Photos/Photos from 2024/alpha.png", b"same")],
    )?;
    let plan = scan(&[first.clone(), identical])?;
    assert_eq!(plan.summary.assets, 1);
    assert!(
        plan.warnings
            .iter()
            .any(|item| item.rule_id == rule_id::TAKEOUT_ARCHIVE_DUPLICATE)
    );

    let conflict = directory.archive(
        "takeout-003.zip",
        &[(
            "Takeout/Google Photos/Photos from 2024/alpha.png",
            b"different",
        )],
    )?;
    let plan = scan(&[first, conflict])?;
    assert_eq!(plan.summary.assets, 0);
    assert!(
        plan.errors
            .iter()
            .any(|item| item.rule_id == rule_id::TAKEOUT_ARCHIVE_CONFLICT)
    );
    Ok(())
}

#[test]
fn archive_traversal_and_compression_bombs_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unsafe")?;
    let traversal = directory.archive("traversal.zip", &[("../escape.png", b"synthetic")])?;
    assert!(matches!(
        scan(&[traversal]),
        Err(ScanError::InvalidArchive(_))
    ));

    let path = directory.0.join("ratio.zip");
    let file = File::create(&path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.start_file(
        "Takeout/Google Photos/Photos from 2024/repeated.png",
        options,
    )?;
    std::io::copy(&mut std::io::repeat(0).take(2_000_000), &mut writer)?;
    writer.finish()?;
    assert!(matches!(scan(&[path]), Err(ScanError::InvalidArchive(_))));
    Ok(())
}
