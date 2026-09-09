use std::fs;
use std::path::{Component, Path, PathBuf};

use immich_rs_application::FolderScanConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::WebConfigError;

const MAX_ID_BYTES: usize = 64;
const MAX_LABEL_BYTES: usize = 128;

mod server;
mod state;

pub use server::{RawServerProfile, ServerMode, ServerProfile};
pub use state::{RawStateProfile, ResolvedStateProfile, StateProfile};

/// Operator-configured folder profile addressed by an opaque browser ID.
pub struct SourceProfile {
    id: String,
    label: String,
    configured_root: PathBuf,
    canonical_root: PathBuf,
    identity: ResourceIdentity,
    generation_sha256: String,
    config: FolderScanConfig,
}

impl SourceProfile {
    pub(crate) fn from_raw(raw: RawSourceProfile) -> Result<Self, WebConfigError> {
        validate_id(&raw.id)?;
        validate_label(&raw.label)?;
        if raw.generation == 0
            || !raw.allowed_root.is_absolute()
            || !valid_relative_root(&raw.relative_root)
        {
            return Err(WebConfigError::new("source profile paths are invalid"));
        }
        let allowed = canonical_directory(&raw.allowed_root)?;
        let configured_root = raw.allowed_root.join(&raw.relative_root);
        let canonical_root = canonical_directory(&configured_root)?;
        if !canonical_root.starts_with(&allowed) {
            return Err(WebConfigError::new(
                "source profile escapes its allowed root",
            ));
        }
        let metadata = fs::metadata(&canonical_root)
            .map_err(|_| WebConfigError::new("cannot inspect source profile"))?;
        let identity = ResourceIdentity::from_metadata(&metadata)?;
        let config = raw.scan.into_config()?;
        let generation_sha256 = source_generation(
            raw.generation,
            &raw.id,
            &raw.label,
            &canonical_root,
            identity,
            &config,
        );
        Ok(Self {
            id: raw.id,
            label: raw.label,
            configured_root,
            canonical_root,
            identity,
            generation_sha256,
            config,
        })
    }

    /// Opaque browser-safe identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Operator-defined non-path display label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Reopen and revalidate the exact configured directory identity at use.
    pub fn resolve(&self) -> Result<ResolvedSourceProfile, WebConfigError> {
        let canonical = canonical_directory(&self.configured_root)?;
        let metadata = fs::metadata(&canonical)
            .map_err(|_| WebConfigError::new("cannot inspect source profile"))?;
        let identity = ResourceIdentity::from_metadata(&metadata)?;
        if canonical != self.canonical_root || identity != self.identity {
            return Err(WebConfigError::new("source profile identity changed"));
        }
        Ok(ResolvedSourceProfile {
            root: canonical,
            label: self.label.clone(),
            generation_sha256: self.generation_sha256.clone(),
            config: self.config.clone(),
        })
    }
}

/// Revalidated source facts passed directly to the application facade.
pub struct ResolvedSourceProfile {
    root: PathBuf,
    label: String,
    generation_sha256: String,
    config: FolderScanConfig,
}

impl ResolvedSourceProfile {
    /// Canonical configured source path, never browser-derived.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Non-path plan label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Digest binding operator generation, identity and scan configuration.
    #[must_use]
    pub fn generation_sha256(&self) -> &str {
        &self.generation_sha256
    }

    /// Fixed scanner bounds from operator configuration.
    #[must_use]
    pub const fn config(&self) -> &FolderScanConfig {
        &self.config
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSourceProfile {
    pub id: String,
    label: String,
    allowed_root: PathBuf,
    relative_root: PathBuf,
    generation: u64,
    #[serde(default)]
    scan: RawFolderScanConfig,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawFolderScanConfig {
    buffer_bytes: Option<usize>,
    max_entries: Option<usize>,
    max_directory_entries: Option<usize>,
    max_path_bytes: Option<usize>,
    case_sensitive: Option<bool>,
}

impl RawFolderScanConfig {
    fn into_config(self) -> Result<FolderScanConfig, WebConfigError> {
        let defaults = FolderScanConfig::default();
        let config = FolderScanConfig {
            buffer_bytes: self.buffer_bytes.unwrap_or(defaults.buffer_bytes),
            max_entries: self.max_entries.unwrap_or(defaults.max_entries),
            max_directory_entries: self
                .max_directory_entries
                .unwrap_or(defaults.max_directory_entries),
            max_path_bytes: self.max_path_bytes.unwrap_or(defaults.max_path_bytes),
            case_sensitive: self.case_sensitive.unwrap_or(defaults.case_sensitive),
        };
        let valid = (4_096..=4 * 1_024 * 1_024).contains(&config.buffer_bytes)
            && config.max_entries > 0
            && config.max_directory_entries > 0
            && (64..=65_536).contains(&config.max_path_bytes);
        valid
            .then_some(config)
            .ok_or_else(|| WebConfigError::new("source scan limits are invalid"))
    }
}

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

fn source_generation(
    generation: u64,
    id: &str,
    label: &str,
    canonical_root: &Path,
    identity: ResourceIdentity,
    config: &FolderScanConfig,
) -> String {
    let mut digest = Sha256::new();
    digest.update(generation.to_le_bytes());
    digest.update(id.as_bytes());
    digest.update(label.as_bytes());
    update_path_digest(&mut digest, canonical_root);
    identity.update_digest(&mut digest);
    digest.update(config.buffer_bytes.to_le_bytes());
    digest.update(config.max_entries.to_le_bytes());
    digest.update(config.max_directory_entries.to_le_bytes());
    digest.update(config.max_path_bytes.to_le_bytes());
    digest.update([u8::from(config.case_sensitive)]);
    format!("{:x}", digest.finalize())
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
