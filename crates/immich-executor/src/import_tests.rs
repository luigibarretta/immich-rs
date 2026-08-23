use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{CancellationToken, NeverCancel, ServerCompatibility, ServerVersion};
use immich_rs_sources::{NoProgress, TakeoutScanConfig, scan_google_takeout_inputs_resolved};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::{
    ExecutorErrorClass, TakeoutImportConfig, create_takeout_upload_plan, dry_run_takeout_import,
};

const MEDIA: &[u8] = b"synthetic image\n";
const OTHER_MEDIA: &[u8] = b"unmatched image\n";
const SIDECAR: &[u8] = br#"{"title":"synthetic.jpg","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200"}}"#;
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TakeoutFixture(PathBuf);

impl TakeoutFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-import-test-{}-{sequence}",
            std::process::id()
        ));
        let photos = root.join("Takeout/Google Photos/Photos from 2024");
        let album = root.join("Takeout/Google Photos/Synthetic album");
        fs::create_dir_all(&photos)?;
        fs::create_dir_all(&album)?;
        fs::write(photos.join("synthetic.jpg"), MEDIA)?;
        fs::write(photos.join("unmatched.jpg"), OTHER_MEDIA)?;
        fs::write(photos.join("synthetic.jpg.json"), SIDECAR)?;
        fs::write(album.join("synthetic.jpg"), MEDIA)?;
        fs::write(album.join("synthetic.jpg.json"), SIDECAR)?;
        fs::write(
            album.join("metadata.json"),
            br#"{"title":"Synthetic album"}"#,
        )?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn archive(&self) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("takeout.zip");
        let mut writer = ZipWriter::new(File::create(&path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in archive_entries() {
            writer.start_file(name, options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for TakeoutFixture {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

fn archive_entries() -> [(&'static str, &'static [u8]); 6] {
    [
        (
            "Takeout/Google Photos/Photos from 2024/synthetic.jpg",
            MEDIA,
        ),
        (
            "Takeout/Google Photos/Photos from 2024/synthetic.jpg.json",
            SIDECAR,
        ),
        (
            "Takeout/Google Photos/Photos from 2024/unmatched.jpg",
            OTHER_MEDIA,
        ),
        ("Takeout/Google Photos/Synthetic album/synthetic.jpg", MEDIA),
        (
            "Takeout/Google Photos/Synthetic album/synthetic.jpg.json",
            SIDECAR,
        ),
        (
            "Takeout/Google Photos/Synthetic album/metadata.json",
            br#"{"title":"Synthetic album"}"#,
        ),
    ]
}

fn server() -> ServerCompatibility {
    ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "a".repeat(64),
    }
}

fn plan_fixture(
    fixture: &TakeoutFixture,
    config: &TakeoutImportConfig,
) -> Result<immich_rs_core::UploadPlan, Box<dyn Error>> {
    let resolved = scan_google_takeout_inputs_resolved(
        &[fixture.path().to_path_buf()],
        "synthetic-takeout",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    Ok(create_takeout_upload_plan(&resolved, server(), config)?)
}

#[test]
fn takeout_plan_and_dry_run_are_exact_for_directory_and_zip() -> Result<(), Box<dyn Error>> {
    let fixture = TakeoutFixture::new()?;
    let config = TakeoutImportConfig::default();
    let directory_inputs = [fixture.path().to_path_buf()];
    let plan = plan_fixture(&fixture, &config)?;
    assert_eq!(plan.summary.max_mutations, 5);
    let archive_inputs = [fixture.archive()?];
    let archived = scan_google_takeout_inputs_resolved(
        &archive_inputs,
        "synthetic-takeout",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    assert_eq!(
        create_takeout_upload_plan(&archived, server(), &config)?,
        plan
    );

    let checkpoint = fixture.path().join("checkpoint.sqlite");
    let directory_report =
        dry_run_takeout_import(&plan, &directory_inputs, &checkpoint, &config, &NeverCancel)?;
    let archive_report =
        dry_run_takeout_import(&plan, &archive_inputs, &checkpoint, &config, &NeverCancel)?;
    assert_eq!(archive_report, directory_report);
    assert_eq!(directory_report.would_upload, 2);
    assert_eq!(directory_report.would_update_metadata, 1);
    assert_eq!(directory_report.would_create_albums, 1);
    assert_eq!(directory_report.would_add_album_memberships, 1);
    assert!(!checkpoint.exists());
    Ok(())
}

#[test]
fn takeout_dry_run_rejects_drift_cancellation_and_existing_checkpoint() -> Result<(), Box<dyn Error>>
{
    let fixture = TakeoutFixture::new()?;
    let config = TakeoutImportConfig::default();
    let plan = plan_fixture(&fixture, &config)?;
    let inputs = [fixture.path().to_path_buf()];
    let checkpoint = fixture.path().join("checkpoint.sqlite");

    let mut drifted = config.clone();
    drifted.upload.concurrency = 2;
    let drift = dry_run_takeout_import(&plan, &inputs, &checkpoint, &drifted, &NeverCancel)
        .err()
        .ok_or("configuration drift was accepted")?;
    assert_eq!(drift.class(), ExecutorErrorClass::InvalidPlan);

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let cancelled = dry_run_takeout_import(&plan, &inputs, &checkpoint, &config, &cancellation)
        .err()
        .ok_or("cancelled dry-run was accepted")?;
    assert_eq!(cancelled.class(), ExecutorErrorClass::Cancelled);

    fs::write(
        fixture
            .path()
            .join("Takeout/Google Photos/Photos from 2024/unmatched.jpg"),
        b"changed synthetic image\n",
    )?;
    let changed = dry_run_takeout_import(&plan, &inputs, &checkpoint, &config, &NeverCancel)
        .err()
        .ok_or("changed Takeout was accepted")?;
    assert_eq!(changed.class(), ExecutorErrorClass::SourceChanged);

    fs::write(&checkpoint, b"synthetic occupied checkpoint\n")?;
    let occupied = dry_run_takeout_import(&plan, &inputs, &checkpoint, &config, &NeverCancel)
        .err()
        .ok_or("existing checkpoint was accepted")?;
    assert_eq!(occupied.class(), ExecutorErrorClass::Checkpoint);
    Ok(())
}
