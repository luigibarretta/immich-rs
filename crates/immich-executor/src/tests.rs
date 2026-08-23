use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_client::RemoteArchiveAsset;
use immich_rs_core::{
    CancellationToken, MediaKind, NeverCancel, ServerCompatibility, ServerVersion,
};
use immich_rs_sources::{NoProgress, scan_folder_resolved};

use crate::journal::{Journal, JournalEvent, OutcomeKind};
use crate::{
    ArchivePlanningConfig, ExecutorErrorClass, UploadExecutionConfig, create_archive_manifest,
    create_upload_plan, dry_run_upload,
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
