use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_client::RemoteArchiveAsset;
use immich_rs_core::{
    CancellationToken, MediaKind, NeverCancel, ServerCompatibility, ServerVersion,
};
use immich_rs_sources::{
    NoProgress, TakeoutScanConfig, scan_folder_resolved, scan_google_takeout_inputs_resolved,
};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::journal::{Journal, JournalEvent, OutcomeKind};
use crate::{
    ArchivePlanningConfig, ExecutorErrorClass, TakeoutImportConfig, UploadExecutionConfig,
    create_archive_manifest, create_takeout_upload_plan, create_upload_plan, dry_run_upload,
};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct SyntheticDirectory(PathBuf);

impl SyntheticDirectory {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-executor-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn archive(&self, entries: &[(&str, &[u8])]) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join("takeout.zip");
        let mut writer = ZipWriter::new(File::create(&path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in entries {
            writer.start_file(*name, options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for SyntheticDirectory {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

fn synthetic_plan(
    directory: &SyntheticDirectory,
    config: &UploadExecutionConfig,
) -> Result<immich_rs_core::UploadPlan, Box<dyn Error>> {
    fs::write(
        directory.path().join("synthetic.jpg"),
        b"synthetic generated image payload\n",
    )?;
    let mut progress = NoProgress;
    let resolved = scan_folder_resolved(
        directory.path(),
        "synthetic-folder",
        &config.scan,
        &NeverCancel,
        &mut progress,
    )?;
    Ok(create_upload_plan(
        &resolved,
        ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "a".repeat(64),
        },
        config,
    )?)
}

#[test]
fn dry_run_verifies_without_creating_a_checkpoint() -> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    let config = UploadExecutionConfig::default();
    let plan = synthetic_plan(&directory, &config)?;
    let checkpoint = directory.path().join("checkpoint.sqlite");

    let report = dry_run_upload(&plan, directory.path(), &checkpoint, &config, &NeverCancel)?;

    assert!(report.dry_run);
    assert_eq!(report.would_upload, 1);
    assert!(!checkpoint.exists());
    Ok(())
}

#[test]
fn checkpoint_resume_is_append_only_and_server_bound() -> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    let config = UploadExecutionConfig::default();
    let plan = synthetic_plan(&directory, &config)?;
    let checkpoint = directory.path().join("checkpoint.sqlite");
    let mut journal = Journal::open(&checkpoint, &plan).map_err(|_| "checkpoint open failed")?;
    let operation = plan.operations.first().ok_or("synthetic plan is empty")?;
    journal
        .append(&JournalEvent {
            operation_id: &operation.operation_id,
            kind: OutcomeKind::Created,
            asset_id: Some("00000000-0000-4000-8000-000000000010"),
            retry_count: 0,
        })
        .map_err(|_| "checkpoint append failed")?;
    drop(journal);

    let report = dry_run_upload(&plan, directory.path(), &checkpoint, &config, &NeverCancel)?;
    assert_eq!(report.resumed, 1);
    assert_eq!(report.would_upload, 0);

    let mut other_server = plan.clone();
    other_server.server.identity_sha256 = "b".repeat(64);
    let error = Journal::validate_existing(&checkpoint, &other_server)
        .err()
        .ok_or("checkpoint mismatch was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::Checkpoint);
    Ok(())
}

#[test]
fn production_checkpoint_binds_backup_digest_and_remains_dry_run_readable()
-> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    let config = UploadExecutionConfig::default();
    let plan = synthetic_plan(&directory, &config)?;
    let checkpoint = directory.path().join("production-checkpoint.sqlite");
    let backup_digest = "b".repeat(64);
    let journal = Journal::open_production(&checkpoint, &plan, &backup_digest)
        .map_err(|_| "production checkpoint open failed")?;
    drop(journal);

    let report = dry_run_upload(&plan, directory.path(), &checkpoint, &config, &NeverCancel)?;
    assert!(report.dry_run);

    let disposable = Journal::open(&checkpoint, &plan)
        .err()
        .ok_or("production checkpoint accepted as disposable")?;
    assert_eq!(disposable.class(), ExecutorErrorClass::Checkpoint);

    let other_backup = Journal::open_production(&checkpoint, &plan, &"c".repeat(64))
        .err()
        .ok_or("different backup digest accepted")?;
    assert_eq!(other_backup.class(), ExecutorErrorClass::Checkpoint);
    Ok(())
}

#[test]
fn source_change_and_cancellation_fail_closed() -> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    let config = UploadExecutionConfig::default();
    let plan = synthetic_plan(&directory, &config)?;
    fs::write(
        directory.path().join("synthetic.jpg"),
        b"changed synthetic payload\n",
    )?;
    let checkpoint = directory.path().join("checkpoint.sqlite");
    let changed = dry_run_upload(&plan, directory.path(), &checkpoint, &config, &NeverCancel)
        .err()
        .ok_or("changed source was accepted")?;
    assert_eq!(changed.class(), ExecutorErrorClass::SourceChanged);

    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let error = dry_run_upload(&plan, directory.path(), &checkpoint, &config, &cancelled)
        .err()
        .ok_or("cancelled verification was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::Cancelled);
    Ok(())
}

#[test]
fn json_sidecar_is_rejected_until_metadata_reconciliation() -> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    fs::write(directory.path().join("synthetic.jpg"), b"synthetic image\n")?;
    fs::write(
        directory.path().join("synthetic.jpg.json"),
        b"{\"synthetic\":true}\n",
    )?;
    let config = UploadExecutionConfig::default();
    let mut progress = NoProgress;
    let resolved = scan_folder_resolved(
        directory.path(),
        "synthetic-folder",
        &config.scan,
        &NeverCancel,
        &mut progress,
    )?;
    let error = create_upload_plan(
        &resolved,
        ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "a".repeat(64),
        },
        &config,
    )
    .err()
    .ok_or("JSON sidecar was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::UnsupportedMetadata);
    Ok(())
}

#[test]
fn takeout_plan_preserves_metadata_and_exact_mutation_budget() -> Result<(), Box<dyn Error>> {
    let directory = SyntheticDirectory::new()?;
    let photos = directory
        .path()
        .join("Takeout/Google Photos/Photos from 2024");
    fs::create_dir_all(&photos)?;
    fs::write(photos.join("synthetic.jpg"), b"synthetic image\n")?;
    fs::write(photos.join("unmatched.jpg"), b"unmatched image\n")?;
    fs::write(
        photos.join("synthetic.jpg.json"),
        br#"{"title":"synthetic.jpg","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200"}}"#,
    )?;
    let album = directory
        .path()
        .join("Takeout/Google Photos/Synthetic album");
    fs::create_dir_all(&album)?;
    fs::write(album.join("synthetic.jpg"), b"synthetic image\n")?;
    fs::write(
        album.join("synthetic.jpg.json"),
        br#"{"title":"synthetic.jpg","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200"}}"#,
    )?;
    fs::write(
        album.join("metadata.json"),
        br#"{"title":"Synthetic album"}"#,
    )?;
    let config = TakeoutImportConfig::default();
    let resolved = scan_google_takeout_inputs_resolved(
        &[directory.path().to_path_buf()],
        "synthetic-takeout",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    let server = ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "a".repeat(64),
    };
    let plan = create_takeout_upload_plan(&resolved, server.clone(), &config)?;
    assert_eq!(plan.schema_version, 2);
    assert_eq!(plan.summary.operations, 2);
    assert_eq!(plan.summary.metadata_updates, 1);
    assert_eq!(plan.summary.album_creates, 1);
    assert_eq!(plan.summary.album_memberships, 1);
    assert_eq!(plan.summary.max_mutations, 5);
    let operation = plan
        .operations
        .iter()
        .find(|operation| operation.relative_path.ends_with("synthetic.jpg"))
        .ok_or("missing operation")?;
    assert_eq!(operation.created_at_unix_ms, 1_704_067_200_000);
    assert_eq!(operation.modified_at_unix_ms, 1_704_067_200_000);
    assert_eq!(
        operation
            .normalized_metadata
            .as_ref()
            .ok_or("missing metadata")?
            .albums,
        ["Synthetic album"]
    );
    let archive = directory.archive(&[
        (
            "Takeout/Google Photos/Photos from 2024/synthetic.jpg",
            b"synthetic image\n",
        ),
        (
            "Takeout/Google Photos/Photos from 2024/synthetic.jpg.json",
            br#"{"title":"synthetic.jpg","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200"}}"#,
        ),
        (
            "Takeout/Google Photos/Photos from 2024/unmatched.jpg",
            b"unmatched image\n",
        ),
        (
            "Takeout/Google Photos/Synthetic album/synthetic.jpg",
            b"synthetic image\n",
        ),
        (
            "Takeout/Google Photos/Synthetic album/synthetic.jpg.json",
            br#"{"title":"synthetic.jpg","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200"}}"#,
        ),
        (
            "Takeout/Google Photos/Synthetic album/metadata.json",
            br#"{"title":"Synthetic album"}"#,
        ),
    ])?;
    let archived = scan_google_takeout_inputs_resolved(
        &[archive],
        "synthetic-takeout",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    let archived_plan = create_takeout_upload_plan(&archived, server, &config)?;
    assert_eq!(archived_plan, plan);
    plan.validate()?;
    Ok(())
}

#[test]
fn archive_manifest_is_sorted_and_configuration_bound() -> Result<(), Box<dyn Error>> {
    let server = ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "a".repeat(64),
    };
    let assets = vec![
        RemoteArchiveAsset {
            asset_id: "00000000-0000-4000-8000-000000000002".to_owned(),
            original_file_name: "synthetic-b.jpg".to_owned(),
            media_kind: MediaKind::Image,
            byte_len: 20,
            checksum_sha1: "b".repeat(40),
        },
        RemoteArchiveAsset {
            asset_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            original_file_name: "synthetic-a.mov".to_owned(),
            media_kind: MediaKind::Video,
            byte_len: 10,
            checksum_sha1: "a".repeat(40),
        },
    ];
    let manifest = create_archive_manifest(assets, server, &ArchivePlanningConfig::default())?;

    assert_eq!(manifest.summary.assets, 2);
    assert_eq!(manifest.summary.media_bytes, 30);
    assert!(manifest.assets[0].target_path.ends_with("synthetic-a.mov"));
    manifest.validate()?;
    Ok(())
}
