use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use super::history::HistoryStore;
use super::{HistoryKind, SafeCounters, TerminalRecord, TerminalStatus};

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
