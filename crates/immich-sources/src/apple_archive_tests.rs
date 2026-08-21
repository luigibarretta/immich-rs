use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{CancellationToken, NeverCancel, rule_id};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::{ApplePhotosScanConfig, NoProgress, ScanError, scan_apple_photos_inputs};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-apple-archive-{name}-{}-{sequence}",
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
        let mut writer = ZipWriter::new(File::create(&path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (entry, bytes) in entries {
            writer.start_file(*entry, options)?;
            writer.write_all(bytes)?;
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
    scan_apple_photos_inputs(
        inputs,
        "synthetic-apple-zip",
        &ApplePhotosScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn split_zip_order_and_entry_order_are_not_semantic() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("order")?;
    let first = directory.archive(
        "icloud-001.zip",
        &[("Album/beta.png", b"beta"), ("Album/alpha.png", b"alpha")],
    )?;
    let second = directory.archive("icloud-002.zip", &[("Album/pair.mov", b"motion")])?;
    let forward = scan(&[first.clone(), second.clone()])?;
    let reverse = scan(&[second, first])?;
    assert_eq!(forward, reverse);
    assert_eq!(forward.summary.assets, 3);
    Ok(())
}

#[test]
fn duplicate_conflict_noise_and_traversal_are_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("safety")?;
    let first = directory.archive("one.zip", &[("Album/image.png", b"same")])?;
    let duplicate = directory.archive("two.zip", &[("Album/image.png", b"same")])?;
    let plan = scan(&[first.clone(), duplicate])?;
    assert!(
        plan.warnings
            .iter()
            .any(|item| item.rule_id == rule_id::APPLE_ARCHIVE_DUPLICATE)
    );
    let conflict = directory.archive("three.zip", &[("Album/image.png", b"different")])?;
    let plan = scan(&[first, conflict])?;
    assert!(
        plan.errors
            .iter()
            .any(|item| item.rule_id == rule_id::APPLE_ARCHIVE_CONFLICT)
    );

    let noise = directory.archive(
        "noise.zip",
        &[
            ("Recently Deleted/image.png", b"noise"),
            ("Album/keep.png", b"keep"),
        ],
    )?;
    let plan = scan(&[noise])?;
    assert_eq!(plan.summary.assets, 1);
    assert_eq!(plan.summary.bytes_read, 4);

    let traversal = directory.archive("traversal.zip", &[("../escape.png", b"escape")])?;
    assert!(matches!(
        scan(&[traversal]),
        Err(ScanError::InvalidArchive(_))
    ));
    Ok(())
}

#[test]
fn cancelled_archive_returns_no_partial_plan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("cancel")?;
    let archive = directory.archive("cancel.zip", &[("Album/image.png", b"image")])?;
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let result = scan_apple_photos_inputs(
        &[archive],
        "synthetic-cancelled",
        &ApplePhotosScanConfig::default(),
        &cancellation,
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)));
    Ok(())
}
