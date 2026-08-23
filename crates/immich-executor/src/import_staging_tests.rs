use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{CancellationToken, NeverCancel, ServerCompatibility, ServerVersion};
use immich_rs_sources::{NoProgress, scan_google_takeout_inputs_resolved};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::import_staging::ImportStaging;
use crate::{ExecutorErrorClass, TakeoutImportConfig, create_takeout_upload_plan};

const MEDIA: &[u8] = b"synthetic staged image\n";
const SIDECAR: &[u8] = br#"{"title":"staged.jpg","description":"synthetic staging"}"#;
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct StagingFixture(PathBuf);

impl StagingFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-staging-test-{}-{sequence}",
            std::process::id()
        ));
        let photos = root.join("Takeout/Google Photos/Photos from 2024");
        fs::create_dir_all(&photos)?;
        fs::write(photos.join("staged.jpg"), MEDIA)?;
        fs::write(photos.join("staged.jpg.json"), SIDECAR)?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn archive(&self) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("takeout.zip");
        let mut writer = ZipWriter::new(File::create(&path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("Takeout/Google Photos/Photos from 2024/staged.jpg", options)?;
        writer.write_all(MEDIA)?;
        writer.start_file(
            "Takeout/Google Photos/Photos from 2024/staged.jpg.json",
            options,
        )?;
        writer.write_all(SIDECAR)?;
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for StagingFixture {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn archive_entry_is_staged_exactly_and_removed_by_ownership() -> Result<(), Box<dyn Error>> {
    let fixture = StagingFixture::new()?;
    let archive = fixture.archive()?;
    let config = TakeoutImportConfig::default();
    let resolved = scan_google_takeout_inputs_resolved(
        std::slice::from_ref(&archive),
        "synthetic-staging",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_takeout_upload_plan(&resolved, server(), &config)?;
    let operation = plan.operations.first().ok_or("missing operation")?;
    let checkpoint = fixture.path().join("checkpoint.sqlite");
    let staging = ImportStaging::open(&checkpoint, &plan, &config)?;
    assert!(staging.root().is_dir());

    let prepared = staging.prepare(&resolved, operation, &NeverCancel)?;
    assert_eq!(fs::read(&prepared.media_path)?, MEDIA);
    assert_eq!(prepared.file_name, "staged.jpg");
    assert!(!prepared.sha1_base64.is_empty());
    assert!(prepared.xmp.is_none());
    assert!(prepared.media_path.starts_with(staging.root()));
    drop(prepared);
    assert_eq!(fs::read_dir(staging.root())?.count(), 0);
    let root = staging.root().to_path_buf();
    drop(staging);
    assert!(!root.exists());
    Ok(())
}

#[test]
fn cancellation_and_archive_drift_leave_no_staged_file() -> Result<(), Box<dyn Error>> {
    let fixture = StagingFixture::new()?;
    let archive = fixture.archive()?;
    let config = TakeoutImportConfig::default();
    let resolved = scan_google_takeout_inputs_resolved(
        std::slice::from_ref(&archive),
        "synthetic-staging",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_takeout_upload_plan(&resolved, server(), &config)?;
    let operation = plan.operations.first().ok_or("missing operation")?;
    let checkpoint = fixture.path().join("checkpoint.sqlite");
    let staging = ImportStaging::open(&checkpoint, &plan, &config)?;
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let error = staging
        .prepare(&resolved, operation, &cancellation)
        .err()
        .ok_or("cancelled staging unexpectedly succeeded")?;
    assert_eq!(error.class(), ExecutorErrorClass::Cancelled);
    assert_eq!(fs::read_dir(staging.root())?.count(), 0);

    fs::write(&archive, b"changed synthetic archive\n")?;
    let error = staging
        .prepare(&resolved, operation, &NeverCancel)
        .err()
        .ok_or("changed archive unexpectedly staged")?;
    assert_eq!(error.class(), ExecutorErrorClass::SourceChanged);
    assert_eq!(fs::read_dir(staging.root())?.count(), 0);
    Ok(())
}

#[test]
fn directory_input_is_never_copied_into_staging() -> Result<(), Box<dyn Error>> {
    let fixture = StagingFixture::new()?;
    let config = TakeoutImportConfig::default();
    let resolved = scan_google_takeout_inputs_resolved(
        &[fixture.path().to_path_buf()],
        "synthetic-staging",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_takeout_upload_plan(&resolved, server(), &config)?;
    let operation = plan.operations.first().ok_or("missing operation")?;
    let staging = ImportStaging::open(&fixture.path().join("checkpoint.sqlite"), &plan, &config)?;
    let prepared = staging.prepare(&resolved, operation, &NeverCancel)?;
    assert_eq!(fs::read(&prepared.media_path)?, MEDIA);
    assert!(!prepared.media_path.starts_with(staging.root()));
    assert_eq!(fs::read_dir(staging.root())?.count(), 0);
    Ok(())
}

fn server() -> ServerCompatibility {
    ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "e".repeat(64),
    }
}
