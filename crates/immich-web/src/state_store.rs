mod history;
#[cfg(test)]
mod tests;
mod types;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use history::HistoryStore;
pub use types::{HistoryKind, SafeCounters, StoredHistory, TerminalRecord, TerminalStatus};

use crate::{ResolvedStateProfile, WebConfigError, WebLimits};

const HISTORY_FILE: &str = "console-history-v1.sqlite3";
const PLANS_DIRECTORY: &str = "plans";
const CHECKPOINTS_DIRECTORY: &str = "checkpoints";

/// Fixed private state layout owned by the Web Console.
pub struct ConsoleStore {
    history: HistoryStore,
    _plans: PathBuf,
    _checkpoints: PathBuf,
    _state_generation_sha256: String,
}

impl ConsoleStore {
    pub fn open(state: &ResolvedStateProfile, limits: WebLimits) -> Result<Self, WebConfigError> {
        let plans = private_directory(state.root(), PLANS_DIRECTORY)?;
        let checkpoints = private_directory(state.root(), CHECKPOINTS_DIRECTORY)?;
        let now = now_unix()?;
        let history = HistoryStore::open(
            &state.root().join(HISTORY_FILE),
            limits.history_store_bytes,
            limits.history_retained_rows,
            limits.history_retention_days,
            now,
        )?;
        Ok(Self {
            history,
            _plans: plans,
            _checkpoints: checkpoints,
            _state_generation_sha256: state.generation_sha256().to_owned(),
        })
    }

    pub const fn history(&self) -> &HistoryStore {
        &self.history
    }
}

pub fn now_unix() -> Result<i64, WebConfigError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WebConfigError::new("system clock is invalid"))?
        .as_secs();
    i64::try_from(now).map_err(|_| WebConfigError::new("system clock is out of range"))
}

fn private_directory(root: &Path, name: &str) -> Result<PathBuf, WebConfigError> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(WebConfigError::new("console state directory is invalid")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_private(&path)?,
        Err(_) => {
            return Err(WebConfigError::new(
                "cannot inspect console state directory",
            ));
        }
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|_| WebConfigError::new("cannot resolve console state directory"))?;
    let root = fs::canonicalize(root)
        .map_err(|_| WebConfigError::new("cannot resolve console state root"))?;
    if !canonical.starts_with(root) || !is_private(&canonical)? {
        return Err(WebConfigError::new(
            "console state directory is not private",
        ));
    }
    Ok(canonical)
}

#[cfg(unix)]
fn create_private(path: &Path) -> Result<(), WebConfigError> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|_| WebConfigError::new("cannot create console state directory"))
}

#[cfg(windows)]
fn create_private(path: &Path) -> Result<(), WebConfigError> {
    fs::create_dir(path).map_err(|_| WebConfigError::new("cannot create console state directory"))
}

#[cfg(unix)]
fn is_private(path: &Path) -> Result<bool, WebConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)
        .map_err(|_| WebConfigError::new("cannot inspect console state permissions"))?;
    Ok(metadata.permissions().mode().trailing_zeros() >= 6)
}

#[cfg(windows)]
fn is_private(_path: &Path) -> Result<bool, WebConfigError> {
    Ok(true)
}
