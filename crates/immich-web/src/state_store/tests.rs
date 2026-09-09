use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use super::history::HistoryStore;
use super::plan_store::PlanStore;
use super::{HistoryKind, SafeCounters, TerminalRecord, TerminalStatus};
use immich_rs_application::UploadPlan;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-web-history-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        make_private(&path)?;
        Ok(Self(path))
    }

    fn database(&self) -> PathBuf {
        self.0.join("console-history-v1.sqlite3")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn terminal_history_is_bounded_and_persists_across_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let store = HistoryStore::open(&workspace.database(), 1024 * 1024, 2, 30, 100)?;
    for recorded_unix in [100, 101, 102] {
        store.record_terminal(&TerminalRecord {
            recorded_unix,
            kind: HistoryKind::FolderPlan,
            status: TerminalStatus::Completed,
            plan: None,
            counters: SafeCounters {
                assets: u64::try_from(recorded_unix)?,
                ..SafeCounters::default()
            },
        })?;
    }
    assert_eq!(store.latest(2)?.len(), 2);
    assert_private_file(&workspace.database())?;
    drop(store);
    let reopened = HistoryStore::open(&workspace.database(), 1024 * 1024, 2, 30, 102)?;
    let latest = reopened.latest(2)?;
    assert_eq!(latest[0].record.counters.assets, 102);
    assert_eq!(latest[1].record.counters.assets, 101);
    Ok(())
}

#[test]
fn newer_corrupt_and_over_bound_databases_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let newer = Workspace::new()?;
    let connection = Connection::open(newer.database())?;
    connection.pragma_update(None, "user_version", 2_i64)?;
    drop(connection);
    assert!(HistoryStore::open(&newer.database(), 1024 * 1024, 10, 30, 1).is_err());

    let corrupt = Workspace::new()?;
    fs::write(corrupt.database(), b"not a sqlite database")?;
    assert!(HistoryStore::open(&corrupt.database(), 1024 * 1024, 10, 30, 1).is_err());

    let bounded = Workspace::new()?;
    assert!(HistoryStore::open(&bounded.database(), 4 * 4096, 10, 30, 1).is_err());
    Ok(())
}

#[test]
fn upload_plans_are_private_immutable_and_digest_bound() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let plans = workspace.0.join("plans");
    fs::create_dir(&plans)?;
    make_private(&plans)?;
    let store = PlanStore::new(plans.clone(), 1024 * 1024, 2 * 1024 * 1024, 8);
    let plan = synthetic_upload_plan()?;
    let artifact = store.write(
        &plan,
        "source-one",
        &"1".repeat(64),
        "server-one",
        &"2".repeat(64),
        3,
    )?;
    let loaded = store.load(artifact.reference)?;
    assert_eq!(loaded.plan, plan);
    assert_eq!(loaded.binding.plan_sha256, artifact.plan_sha256);
    assert_eq!(loaded.binding.max_logical_effects, 1);
    assert_eq!(loaded.reference.encode().len(), 22);

    let plan_path = plans.join(format!("{}.json", artifact.reference.encode()));
    let binding = plans.join(format!("{}.binding.json", artifact.reference.encode()));
    assert_private_file(&plan_path)?;
    assert_private_file(&binding)?;
    let reopened = PlanStore::new(plans, 1024 * 1024, 2 * 1024 * 1024, 8);
    assert_eq!(reopened.load(artifact.reference)?.plan, plan);
    let contents = fs::read_to_string(&binding)?.replace(&artifact.plan_sha256, &"f".repeat(64));
    fs::write(&binding, contents)?;
    assert!(store.load(artifact.reference).is_err());
    Ok(())
}

#[test]
fn plan_store_bound_fails_without_partial_artifact() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let plans = workspace.0.join("plans");
    fs::create_dir(&plans)?;
    make_private(&plans)?;
    let store = PlanStore::new(plans.clone(), 1024 * 1024, 1, 8);
    assert!(
        store
            .write(
                &synthetic_upload_plan()?,
                "source-one",
                &"1".repeat(64),
                "server-one",
                &"2".repeat(64),
                3,
            )
            .is_err()
    );
    assert_eq!(fs::read_dir(plans)?.count(), 0);
    Ok(())
}

fn synthetic_upload_plan() -> Result<UploadPlan, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "normalized_plan_sha256": "3".repeat(64),
        "source": {
            "kind": "folder",
            "label": "synthetic-source",
            "fingerprint_sha256": "4".repeat(64),
            "case_sensitive": true,
            "unicode_normalization": "nfc"
        },
        "configuration_sha256": "5".repeat(64),
        "server": {
            "version": {"major": 3, "minor": 1, "patch": 0},
            "identity_sha256": "6".repeat(64)
        },
        "operations": [{
            "operation_id": "7".repeat(64),
            "relative_path": "synthetic.jpg",
            "media_kind": "image",
            "byte_len": 16,
            "content_sha256": "8".repeat(64),
            "created_at_unix_ms": 1,
            "modified_at_unix_ms": 1,
            "xmp_sidecar": null,
            "role": {"kind": "standalone"}
        }],
        "summary": {
            "operations": 1,
            "media_bytes": 16,
            "xmp_sidecars": 0,
            "live_photo_pairs": 0
        }
    }))?)
}

#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn assert_private_file(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    assert!(fs::metadata(path)?.permissions().mode().trailing_zeros() >= 6);
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(windows)]
fn assert_private_file(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
