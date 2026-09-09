use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    ResourceIdentity, canonical_directory, update_path_digest, valid_relative_root, validate_id,
    validate_label,
};
use crate::WebConfigError;

/// Private operator state root addressed by an opaque browser ID.
pub struct StateProfile {
    id: String,
    label: String,
    configured_root: PathBuf,
    canonical_root: PathBuf,
    identity: ResourceIdentity,
    generation_sha256: String,
}

impl StateProfile {
    pub(crate) fn from_raw(raw: RawStateProfile) -> Result<Self, WebConfigError> {
        validate_id(&raw.id)?;
        validate_label(&raw.label)?;
        if raw.generation == 0
            || !raw.allowed_root.is_absolute()
            || !valid_relative_root(&raw.relative_root)
        {
            return Err(WebConfigError::new("state profile paths are invalid"));
        }
        let allowed = canonical_directory(&raw.allowed_root)?;
        let configured_root = raw.allowed_root.join(&raw.relative_root);
        let canonical_root = canonical_directory(&configured_root)?;
        if !canonical_root.starts_with(&allowed) || !private_directory(&canonical_root)? {
            return Err(WebConfigError::new("state profile root is not private"));
        }
        let metadata = fs::metadata(&canonical_root)
            .map_err(|_| WebConfigError::new("cannot inspect state profile"))?;
        let identity = ResourceIdentity::from_metadata(&metadata)?;
        let generation_sha256 = state_generation(
            raw.generation,
            &raw.id,
            &raw.label,
            &canonical_root,
            identity,
        );
        Ok(Self {
            id: raw.id,
            label: raw.label,
            configured_root,
            canonical_root,
            identity,
            generation_sha256,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn resolve(&self) -> Result<ResolvedStateProfile, WebConfigError> {
        let canonical = canonical_directory(&self.configured_root)?;
        let metadata = fs::metadata(&canonical)
            .map_err(|_| WebConfigError::new("cannot inspect state profile"))?;
        let identity = ResourceIdentity::from_metadata(&metadata)?;
        if canonical != self.canonical_root
            || identity != self.identity
            || !private_directory(&canonical)?
        {
            return Err(WebConfigError::new("state profile identity changed"));
        }
        Ok(ResolvedStateProfile {
            root: canonical,
            generation_sha256: self.generation_sha256.clone(),
        })
    }
}

pub struct ResolvedStateProfile {
    root: PathBuf,
    generation_sha256: String,
}

impl ResolvedStateProfile {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn generation_sha256(&self) -> &str {
        &self.generation_sha256
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStateProfile {
    pub id: String,
    label: String,
    allowed_root: PathBuf,
    relative_root: PathBuf,
    generation: u64,
}

fn state_generation(
    generation: u64,
    id: &str,
    label: &str,
    canonical_root: &Path,
    identity: ResourceIdentity,
) -> String {
    let mut digest = Sha256::new();
    digest.update(generation.to_le_bytes());
    digest.update(id.as_bytes());
    digest.update(label.as_bytes());
    update_path_digest(&mut digest, canonical_root);
    identity.update_digest(&mut digest);
    format!("{:x}", digest.finalize())
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<bool, WebConfigError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path)
        .map_err(|_| WebConfigError::new("cannot inspect state profile permissions"))?;
    Ok(metadata.permissions().mode().trailing_zeros() >= 6)
}

#[cfg(windows)]
fn private_directory(_path: &Path) -> Result<bool, WebConfigError> {
    Ok(true)
}
