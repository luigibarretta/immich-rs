use std::fs;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::WebConfigError;

const MAX_ID_BYTES: usize = 64;
const MAX_LABEL_BYTES: usize = 128;

#[path = "profiles/secret.rs"]
mod protected_file;
mod server;
mod source;
mod source_config;
mod state;

pub use server::{RawServerProfile, ServerMode, ServerProfile};
pub use source::{RawSourceProfile, ResolvedSourceProfile, SourceProfile};
pub use source_config::{SourceKind, SourceSettings};
pub use state::{RawStateProfile, ResolvedStateProfile, StateProfile};

pub fn canonical_directory(path: &Path) -> Result<PathBuf, WebConfigError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot inspect configured directory"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WebConfigError::new(
            "configured directory must not be a symlink",
        ));
    }
    fs::canonicalize(path).map_err(|_| WebConfigError::new("cannot resolve configured directory"))
}

pub fn canonical_resource(path: &Path) -> Result<(PathBuf, ResourceIdentity), WebConfigError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot inspect configured source input"))?;
    if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(WebConfigError::new(
            "source input must be a regular file or directory",
        ));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|_| WebConfigError::new("cannot resolve configured source input"))?;
    Ok((canonical, ResourceIdentity::from_metadata(&metadata)?))
}

pub fn valid_relative_root(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

pub fn validate_id(value: &str) -> Result<(), WebConfigError> {
    let mut bytes = value.bytes();
    let first = bytes.next();
    let valid = (1..=MAX_ID_BYTES).contains(&value.len())
        && first.is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    valid
        .then_some(())
        .ok_or_else(|| WebConfigError::new("profile identifier is invalid"))
}

pub fn validate_label(value: &str) -> Result<(), WebConfigError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_LABEL_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control);
    valid
        .then_some(())
        .ok_or_else(|| WebConfigError::new("source profile label is invalid"))
}

#[cfg(unix)]
pub fn update_path_digest(digest: &mut Sha256, path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    digest.update(path.as_os_str().as_bytes());
}

#[cfg(windows)]
pub fn update_path_digest(digest: &mut Sha256, path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    for unit in path.as_os_str().encode_wide() {
        digest.update(unit.to_le_bytes());
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ResourceIdentity {
    first: u64,
    second: u64,
}

impl ResourceIdentity {
    #[cfg(unix)]
    pub fn from_metadata(metadata: &fs::Metadata) -> Result<Self, WebConfigError> {
        use std::os::unix::fs::MetadataExt;
        let identity = Self {
            first: metadata.dev(),
            second: metadata.ino(),
        };
        (identity.second != 0)
            .then_some(identity)
            .ok_or_else(|| WebConfigError::new("source identity is unavailable"))
    }

    #[cfg(windows)]
    pub fn from_metadata(metadata: &fs::Metadata) -> Result<Self, WebConfigError> {
        use std::os::windows::fs::MetadataExt;
        let first = metadata
            .volume_serial_number()
            .map(u64::from)
            .ok_or_else(|| WebConfigError::new("source identity is unavailable"))?;
        let second = metadata
            .file_index()
            .ok_or_else(|| WebConfigError::new("source identity is unavailable"))?;
        Ok(Self { first, second })
    }

    pub fn update_digest(self, digest: &mut Sha256) {
        digest.update(self.first.to_le_bytes());
        digest.update(self.second.to_le_bytes());
    }
}
