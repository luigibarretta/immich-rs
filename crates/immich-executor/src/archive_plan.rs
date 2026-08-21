use immich_rs_client::RemoteArchiveAsset;
use immich_rs_core::{
    ARCHIVE_MANIFEST_SCHEMA_VERSION, ArchiveAsset, ArchiveManifest, ArchiveManifestSummary,
    ServerCompatibility,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass};

/// Explicit visibility selection for one immutable archive inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveSelection {
    /// Timeline assets only.
    Timeline,
    /// Archived assets only.
    Archive,
    /// Hidden assets only.
    Hidden,
    /// Timeline, archived and hidden assets.
    All,
}

/// Stable selection and resource limits bound into an archive manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ArchivePlanningConfig {
    /// Selected visibility set.
    pub selection: ArchiveSelection,
    /// Include assets currently in trash.
    pub include_trashed: bool,
    /// Maximum results requested per server page.
    pub page_size: usize,
    /// Maximum total selected assets.
    pub max_assets: usize,
}

impl Default for ArchivePlanningConfig {
    fn default() -> Self {
        Self {
            selection: ArchiveSelection::Timeline,
            include_trashed: false,
            page_size: 100,
            max_assets: 100_000,
        }
    }
}

impl ArchivePlanningConfig {
    /// Validate public resource bounds.
    pub const fn validate(&self) -> Result<(), ExecutorError> {
        if matches!(self.page_size, 1..=1_000) && self.max_assets > 0 {
            Ok(())
        } else {
            Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))
        }
    }
}

/// Build a deterministic server-bound archive manifest from read-only facts.
pub fn create_archive_manifest(
    remote_assets: Vec<RemoteArchiveAsset>,
    server: ServerCompatibility,
    config: &ArchivePlanningConfig,
) -> Result<ArchiveManifest, ExecutorError> {
    config.validate()?;
    if remote_assets.len() > config.max_assets
        || server.version.major != 3
        || server.version.minor != 1
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let mut assets = remote_assets
        .into_iter()
        .map(|remote| ArchiveAsset {
            target_path: format!("assets/{}/{}", remote.asset_id, remote.original_file_name),
            asset_id: remote.asset_id,
            original_file_name: remote.original_file_name,
            media_kind: remote.media_kind,
            byte_len: remote.byte_len,
            checksum_sha1: remote.checksum_sha1,
        })
        .collect::<Vec<_>>();
    assets.sort_by(|left, right| left.target_path.cmp(&right.target_path));
    let summary = ArchiveManifestSummary {
        assets: assets.len() as u64,
        media_bytes: assets
            .iter()
            .try_fold(0_u64, |total, asset| total.checked_add(asset.byte_len))
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?,
    };
    let configuration_bytes = serde_json::to_vec(config)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let manifest = ArchiveManifest {
        schema_version: ARCHIVE_MANIFEST_SCHEMA_VERSION,
        server,
        configuration_sha256: format!("{:x}", Sha256::digest(configuration_bytes)),
        assets,
        summary,
    };
    manifest
        .validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    Ok(manifest)
}
