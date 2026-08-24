use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NeverCancel, ServerCompatibility, ServerVersion, SourceKind};
use immich_rs_sources::{AlbumMode, NoProgress, scan_apple_photos_inputs_resolved};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::{
    ApplePhotosImportConfig, ExecutorErrorClass, create_apple_photos_upload_plan,
    dry_run_apple_photos_import,
};

const IMAGE: &[u8] = b"synthetic apple image\n";
const MOTION: &[u8] = b"synthetic apple motion\n";
const XMP: &[u8] = b"<x:xmpmeta>synthetic</x:xmpmeta>\n";
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct AppleFixture(PathBuf);

impl AppleFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-apple-import-{}-{sequence}",
            std::process::id()
        ));
        let album = root.join("Trips/Synthetic");
        fs::create_dir_all(&album)?;
        fs::write(album.join("pair.heic"), IMAGE)?;
        fs::write(album.join("pair.mov"), MOTION)?;
        fs::write(album.join("pair.xmp"), XMP)?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn archive(&self) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("icloud.zip");
        let mut writer = ZipWriter::new(File::create(&path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in [
            ("Trips/Synthetic/pair.heic", IMAGE),
            ("Trips/Synthetic/pair.mov", MOTION),
            ("Trips/Synthetic/pair.xmp", XMP),
        ] {
            writer.start_file(name, options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for AppleFixture {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn apple_directory_and_zip_plans_each_verify_exactly() -> Result<(), Box<dyn Error>> {
    let fixture = AppleFixture::new()?;
    let mut config = ApplePhotosImportConfig::default();
    config.source.album_mode = AlbumMode::Path;
    let directory_inputs = [fixture.path().to_path_buf()];
    let resolved = scan_apple_photos_inputs_resolved(
        &directory_inputs,
        "synthetic-apple-import",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_apple_photos_upload_plan(&resolved, server(), &config)?;
    assert_eq!(plan.source.kind, SourceKind::ApplePhotos);
    assert_eq!(plan.summary.operations, 2);
    assert_eq!(plan.summary.max_mutations, 4);

    let archive_inputs = [fixture.archive()?];
    let archived = scan_apple_photos_inputs_resolved(
        &archive_inputs,
        "synthetic-apple-import",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let archived_plan = create_apple_photos_upload_plan(&archived, server(), &config)?;
    assert_eq!(
        archived_plan.normalized_plan_sha256,
        plan.normalized_plan_sha256
    );
    assert_eq!(archived_plan.summary, plan.summary);
    let checkpoint = fixture.path().join("checkpoint.sqlite");
    let directory_report =
        dry_run_apple_photos_import(&plan, &directory_inputs, &checkpoint, &config, &NeverCancel)?;
    let archive_report = dry_run_apple_photos_import(
        &archived_plan,
        &archive_inputs,
        &checkpoint,
        &config,
        &NeverCancel,
    )?;
    assert_eq!(archive_report, directory_report);
    assert_eq!(directory_report.would_upload, 2);
    assert_eq!(directory_report.would_create_albums, 1);
    assert_eq!(directory_report.would_add_album_memberships, 1);
    assert!(!checkpoint.exists());
    Ok(())
}

#[test]
fn apple_dry_run_rejects_configuration_and_source_drift() -> Result<(), Box<dyn Error>> {
    let fixture = AppleFixture::new()?;
    let config = ApplePhotosImportConfig::default();
    let inputs = [fixture.path().to_path_buf()];
    let resolved = scan_apple_photos_inputs_resolved(
        &inputs,
        "synthetic-apple-drift",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_apple_photos_upload_plan(&resolved, server(), &config)?;
    let checkpoint = fixture.path().join("checkpoint.sqlite");

    let mut drifted = config.clone();
    drifted.source.album_mode = AlbumMode::Folder;
    let error = dry_run_apple_photos_import(&plan, &inputs, &checkpoint, &drifted, &NeverCancel)
        .err()
        .ok_or("configuration drift was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::InvalidPlan);

    fs::write(fixture.path().join("Trips/Synthetic/pair.heic"), b"changed")?;
    let error = dry_run_apple_photos_import(&plan, &inputs, &checkpoint, &config, &NeverCancel)
        .err()
        .ok_or("source drift was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::SourceChanged);
    Ok(())
}

fn server() -> ServerCompatibility {
    ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "b".repeat(64),
    }
}
