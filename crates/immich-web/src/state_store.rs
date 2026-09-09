mod history;
mod plan_io;
mod plan_store;
#[cfg(test)]
mod tests;
mod types;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use history::HistoryStore;
use plan_store::PlanStore;
use sha2::{Digest, Sha256};
pub use types::{
    ArtifactRef, DryRunBinding, DryRunReceipt, HistoryKind, PlanArtifact, PlanBinding, ReceiptRef,
    SafeCounters, StoredHistory, StoredUploadPlan, TerminalRecord, TerminalStatus,
};

use crate::{ResolvedStateProfile, WebConfigError, WebLimits};

const HISTORY_FILE: &str = "console-history-v1.sqlite3";
const PLANS_DIRECTORY: &str = "plans";
const CHECKPOINTS_DIRECTORY: &str = "checkpoints";

/// Fixed private state layout owned by the Web Console.
pub struct ConsoleStore {
    history: HistoryStore,
    plans: PlanStore,
    checkpoints: PathBuf,
    state_generation_sha256: String,
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
        let plans = PlanStore::new(
            plans,
            limits.plan_file_bytes,
            limits.plan_store_bytes,
            limits
                .history_retained_rows
                .saturating_mul(2)
                .saturating_add(16),
        );
        plans.validate()?;
        Ok(Self {
            history,
            plans,
            checkpoints,
            state_generation_sha256: state.generation_sha256().to_owned(),
        })
    }

    pub const fn history(&self) -> &HistoryStore {
        &self.history
    }

    pub const fn plans(&self) -> &PlanStore {
        &self.plans
    }

    pub fn matches_state(&self, state: &ResolvedStateProfile) -> bool {
        state.generation_sha256() == self.state_generation_sha256
            && self.plans.is_below(state.root())
    }

    pub fn unused_checkpoint(&self) -> Result<PathBuf, WebConfigError> {
        let path = self.checkpoints.join(format!(
            "dry-run-{}.sqlite3",
            ArtifactRef::random()?.encode()
        ));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
            _ => Err(WebConfigError::new(
                "dry-run checkpoint path is unavailable",
            )),
        }
    }

    pub fn verify_checkpoint_unused(&self, path: &Path) -> Result<(), WebConfigError> {
        if path.parent() != Some(self.checkpoints.as_path()) {
            return Err(WebConfigError::new("dry-run checkpoint path is invalid"));
        }
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                fs::remove_file(path)
                    .map_err(|_| WebConfigError::new("cannot remove dry-run checkpoint"))?;
                Err(WebConfigError::new(
                    "dry-run created an unexpected checkpoint",
                ))
            }
            _ => Err(WebConfigError::new("dry-run checkpoint path is invalid")),
        }
    }

    pub fn apply_checkpoint(&self, reference: ArtifactRef) -> Result<PathBuf, WebConfigError> {
        let path = self.apply_checkpoint_candidate(reference)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && is_private(&path)? =>
            {
                Ok(path)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_private_file(&path)?;
                Ok(path)
            }
            _ => Err(WebConfigError::new("apply checkpoint is invalid")),
        }
    }

    pub fn apply_checkpoint_candidate(
        &self,
        reference: ArtifactRef,
    ) -> Result<PathBuf, WebConfigError> {
        let path = self
            .checkpoints
            .join(format!("apply-{}.sqlite3", reference.encode()));
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && is_private(&path)? =>
            {
                Ok(path)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
            _ => Err(WebConfigError::new("apply checkpoint is invalid")),
        }
    }

    pub fn receipt_is_newer_than_checkpoint(
        &self,
        reference: ArtifactRef,
        completed_unix: i64,
    ) -> Result<bool, WebConfigError> {
        let path = self
            .checkpoints
            .join(format!("apply-{}.sqlite3", reference.encode()));
        let metadata = match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && is_private(&path)? =>
            {
                metadata
            }
            _ => return Err(WebConfigError::new("apply checkpoint is invalid")),
        };
        let modified = metadata
            .modified()
            .and_then(|value| {
                value
                    .duration_since(UNIX_EPOCH)
                    .map_err(std::io::Error::other)
            })
            .map_err(|_| WebConfigError::new("apply checkpoint time is invalid"))?
            .as_secs();
        let completed = u64::try_from(completed_unix)
            .map_err(|_| WebConfigError::new("dry-run receipt time is invalid"))?;
        Ok(completed > modified)
    }
}

pub fn now_unix() -> Result<i64, WebConfigError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WebConfigError::new("system clock is invalid"))?
        .as_secs();
    i64::try_from(now).map_err(|_| WebConfigError::new("system clock is out of range"))
}

pub fn source_configuration_digest(source: &str, configuration: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"immich-rs-web-source-configuration-v1\0");
    digest.update(source.as_bytes());
    digest.update([0]);
    digest.update(configuration.as_bytes());
    format!("{:x}", digest.finalize())
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

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<(), WebConfigError> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map(|_| ())
        .map_err(|_| WebConfigError::new("cannot create apply checkpoint"))
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<(), WebConfigError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|_| WebConfigError::new("cannot create apply checkpoint"))
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
