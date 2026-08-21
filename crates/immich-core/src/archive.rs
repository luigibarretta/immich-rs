use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    ARCHIVE_APPLY_REPORT_SCHEMA_VERSION, ARCHIVE_MANIFEST_SCHEMA_VERSION, MediaKind,
    ServerCompatibility,
};

/// One immutable original asset selected from Immich.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveAsset {
    /// Canonical Immich asset UUID.
    pub asset_id: String,
    /// Original file name as returned by Immich.
    pub original_file_name: String,
    /// Deterministic portable destination below the archive root.
    pub target_path: String,
    /// Source media family.
    pub media_kind: MediaKind,
    /// Exact original byte length from Immich metadata.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-1 returned by Immich.
    pub checksum_sha1: String,
}

/// Deterministic counters for an immutable archive manifest.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveManifestSummary {
    /// Selected original assets.
    pub assets: u64,
    /// Sum of selected original byte lengths.
    pub media_bytes: u64,
}

/// Versioned read-only inventory consumed by local archive apply.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveManifest {
    /// Archive manifest schema version.
    pub schema_version: u32,
    /// Authenticated server compatibility binding.
    pub server: ServerCompatibility,
    /// SHA-256 of selection and limit configuration.
    pub configuration_sha256: String,
    /// Assets sorted by target path.
    pub assets: Vec<ArchiveAsset>,
    /// Deterministic inventory counters.
    pub summary: ArchiveManifestSummary,
}

impl ArchiveManifest {
    /// Validate schema, server binding, paths, checksums and summary.
    pub fn validate(&self) -> Result<(), ArchiveManifestValidationError> {
        if self.schema_version != ARCHIVE_MANIFEST_SCHEMA_VERSION {
            return Err(ArchiveManifestValidationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if !is_sha256(&self.configuration_sha256)
            || !is_sha256(&self.server.identity_sha256)
            || self.server.version.major != 3
            || self.server.version.minor != 1
        {
            return Err(ArchiveManifestValidationError::InvalidIdentity);
        }
        let mut ids = BTreeSet::new();
        let mut previous_path: Option<&str> = None;
        let mut bytes = 0_u64;
        for asset in &self.assets {
            validate_asset(asset, previous_path, &mut ids)?;
            bytes = bytes
                .checked_add(asset.byte_len)
                .ok_or(ArchiveManifestValidationError::SummaryMismatch)?;
            previous_path = Some(asset.target_path.as_str());
        }
        if self.summary
            != (ArchiveManifestSummary {
                assets: self.assets.len() as u64,
                media_bytes: bytes,
            })
        {
            return Err(ArchiveManifestValidationError::SummaryMismatch);
        }
        Ok(())
    }
}

fn validate_asset<'a>(
    asset: &'a ArchiveAsset,
    previous_path: Option<&str>,
    ids: &mut BTreeSet<&'a str>,
) -> Result<(), ArchiveManifestValidationError> {
    if !canonical_uuid_v4(&asset.asset_id)
        || !safe_component(&asset.original_file_name)
        || !is_sha1(&asset.checksum_sha1)
        || asset.target_path != format!("assets/{}/{}", asset.asset_id, asset.original_file_name)
    {
        return Err(ArchiveManifestValidationError::InvalidAsset);
    }
    if !ids.insert(asset.asset_id.as_str()) {
        return Err(ArchiveManifestValidationError::DuplicateAssetId);
    }
    if previous_path.is_some_and(|path| path >= asset.target_path.as_str()) {
        return Err(ArchiveManifestValidationError::AssetsNotStrictlySorted);
    }
    Ok(())
}

fn safe_component(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !(matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && matches!(&stem[..3], "COM" | "LPT")
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0'))
}

fn canonical_uuid_v4(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
        })
}

fn is_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Stable result of one idempotent local archive apply.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveApplyReport {
    /// Apply report schema version.
    pub schema_version: u32,
    /// SHA-256 of the compact immutable manifest.
    pub manifest_sha256: String,
    /// Originals downloaded and atomically committed in this run.
    pub downloaded: u64,
    /// Existing originals revalidated and reused.
    pub already_complete: u64,
    /// Original bytes written in this run.
    pub bytes_written: u64,
    /// Bounded HTTP retries observed in this run.
    pub retries: u64,
}

impl ArchiveApplyReport {
    /// Validate stable report counters and identity.
    pub fn validate(&self, expected_assets: u64) -> Result<(), ArchiveManifestValidationError> {
        if self.schema_version != ARCHIVE_APPLY_REPORT_SCHEMA_VERSION
            || !is_sha256(&self.manifest_sha256)
            || self.downloaded.saturating_add(self.already_complete) != expected_assets
        {
            return Err(ArchiveManifestValidationError::InvalidReport);
        }
        Ok(())
    }
}

/// Reason an immutable archive contract failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveManifestValidationError {
    /// The manifest schema is unsupported.
    UnsupportedSchema(u32),
    /// A configuration or server identity is malformed.
    InvalidIdentity,
    /// An asset UUID, filename, path, size or checksum is malformed.
    InvalidAsset,
    /// Two assets share a server identifier.
    DuplicateAssetId,
    /// Assets are not strictly ordered by destination path.
    AssetsNotStrictlySorted,
    /// Summary counters disagree with the asset inventory.
    SummaryMismatch,
    /// An apply report violates its stable contract.
    InvalidReport,
}

impl Display for ArchiveManifestValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported archive manifest schema {version}")
            }
            Self::InvalidIdentity => formatter.write_str("invalid archive manifest identity"),
            Self::InvalidAsset => formatter.write_str("invalid archive asset"),
            Self::DuplicateAssetId => formatter.write_str("duplicate archive asset ID"),
            Self::AssetsNotStrictlySorted => {
                formatter.write_str("archive assets are not strictly sorted")
            }
            Self::SummaryMismatch => formatter.write_str("archive manifest summary mismatch"),
            Self::InvalidReport => formatter.write_str("invalid archive apply report"),
        }
    }
}

impl Error for ArchiveManifestValidationError {}
