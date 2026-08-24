use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    MIGRATION_PLAN_SCHEMA_VERSION, MediaKind, NormalizedMetadata, ServerCompatibility, UploadRole,
};

/// Privacy-safe identity for one endpoint participating in a migration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationServer {
    /// Exact API compatibility and authenticated user binding.
    pub compatibility: ServerCompatibility,
    /// SHA-256 of the canonical credential-free server origin.
    pub origin_sha256: String,
}

/// One source asset and the exact destination facts derived from it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationAsset {
    /// Source Immich asset UUID.
    pub source_asset_id: String,
    /// Stable destination operation identity.
    pub operation_id: String,
    /// Portable original filename.
    pub original_file_name: String,
    /// Original media family.
    pub media_kind: MediaKind,
    /// Verified original length.
    pub byte_len: u64,
    /// Verified lowercase hexadecimal SHA-1 from the source inventory and body.
    pub checksum_sha1: String,
    /// Verified lowercase hexadecimal SHA-256 of the streamed original.
    pub content_sha256: String,
    /// Original file creation instant in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Original file modification instant in Unix milliseconds.
    pub modified_at_unix_ms: i64,
    /// Supported source metadata, excluding album membership.
    pub normalized_metadata: Option<NormalizedMetadata>,
    /// Standalone or Live Photo dependency role.
    pub role: UploadRole,
}

/// One exact owned-album projection onto migrated assets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationAlbum {
    /// Exact source album name used for destination reconciliation.
    pub name: String,
    /// Sorted destination operation IDs belonging to the album.
    pub member_operation_ids: Vec<String>,
}

/// Deterministic migration counters and maximum destination mutation budget.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlanSummary {
    /// Planned source assets.
    pub assets: u64,
    /// Verified original media bytes.
    pub media_bytes: u64,
    /// Planned supported metadata assignment requests.
    pub metadata_updates: u64,
    /// Maximum album-create requests.
    pub album_creates: u64,
    /// Planned album-membership requests.
    pub album_memberships: u64,
    /// Maximum destination mutations authorized by this plan.
    pub max_mutations: u64,
}

/// Immutable two-server migration contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    /// Migration-plan schema version.
    pub schema_version: u32,
    /// Source server identity; it never grants mutation capability.
    pub source_server: MigrationServer,
    /// Destination server identity observed through a read-only probe.
    pub destination_server: MigrationServer,
    /// SHA-256 over every accepted source inventory fact.
    pub source_fingerprint_sha256: String,
    /// SHA-256 over compatibility-affecting resource limits and choices.
    pub configuration_sha256: String,
    /// Assets sorted by source asset UUID.
    pub assets: Vec<MigrationAsset>,
    /// Owned albums sorted by exact name.
    pub albums: Vec<MigrationAlbum>,
    /// Deterministic counters and mutation ceiling.
    pub summary: MigrationPlanSummary,
}

impl MigrationPlan {
    /// Validate identities, ordering, dependencies, metadata and counters.
    pub fn validate(&self) -> Result<(), MigrationPlanValidationError> {
        if self.schema_version != MIGRATION_PLAN_SCHEMA_VERSION {
            return Err(MigrationPlanValidationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if !server_valid(&self.source_server)
            || !server_valid(&self.destination_server)
            || self.source_server.origin_sha256 == self.destination_server.origin_sha256
            || self.source_server.compatibility.identity_sha256
                == self.destination_server.compatibility.identity_sha256
            || !lower_hex(&self.source_fingerprint_sha256, 64)
            || !lower_hex(&self.configuration_sha256, 64)
        {
            return Err(MigrationPlanValidationError::InvalidIdentity);
        }
        let operation_map = validate_assets(&self.assets)?;
        validate_albums(&self.albums, &operation_map)?;
        if expected_summary(&self.assets, &self.albums) != self.summary {
            return Err(MigrationPlanValidationError::SummaryMismatch);
        }
        Ok(())
    }
}

fn validate_assets(
    assets: &[MigrationAsset],
) -> Result<BTreeMap<&str, &UploadRole>, MigrationPlanValidationError> {
    let mut previous: Option<&str> = None;
    let mut operations = BTreeMap::new();
    for asset in assets {
        if previous.is_some_and(|value| value >= asset.source_asset_id.as_str()) {
            return Err(MigrationPlanValidationError::AssetsNotStrictlySorted);
        }
        if !uuid_v4(&asset.source_asset_id)
            || !lower_hex(&asset.operation_id, 64)
            || !portable_file_name(&asset.original_file_name)
            || asset.byte_len == 0
            || !lower_hex(&asset.checksum_sha1, 40)
            || !lower_hex(&asset.content_sha256, 64)
            || !valid_unix_ms(asset.created_at_unix_ms)
            || !valid_unix_ms(asset.modified_at_unix_ms)
            || asset
                .normalized_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.is_valid() || !metadata.albums.is_empty())
            || operations
                .insert(asset.operation_id.as_str(), &asset.role)
                .is_some()
        {
            return Err(MigrationPlanValidationError::InvalidAsset);
        }
        previous = Some(asset.source_asset_id.as_str());
    }
    for asset in assets {
        if let UploadRole::LivePhotoImage {
            pair_id,
            video_operation_id,
        } = &asset.role
        {
            match operations.get(video_operation_id.as_str()) {
                Some(UploadRole::LivePhotoVideo {
                    pair_id: video_pair,
                }) if video_pair == pair_id => {}
                _ => return Err(MigrationPlanValidationError::InvalidLivePhotoDependency),
            }
        }
    }
    Ok(operations)
}

fn validate_albums(
    albums: &[MigrationAlbum],
    operations: &BTreeMap<&str, &UploadRole>,
) -> Result<(), MigrationPlanValidationError> {
    let mut previous: Option<&str> = None;
    for album in albums {
        let metadata = NormalizedMetadata {
            albums: vec![album.name.clone()],
            ..NormalizedMetadata::default()
        };
        if previous.is_some_and(|value| value >= album.name.as_str())
            || !metadata.is_valid()
            || album.member_operation_ids.is_empty()
            || !album
                .member_operation_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || album
                .member_operation_ids
                .iter()
                .any(|operation| !operations.contains_key(operation.as_str()))
        {
            return Err(MigrationPlanValidationError::InvalidAlbum);
        }
        previous = Some(album.name.as_str());
    }
    Ok(())
}

fn expected_summary(assets: &[MigrationAsset], albums: &[MigrationAlbum]) -> MigrationPlanSummary {
    let metadata_updates = assets
        .iter()
        .filter(|asset| asset.normalized_metadata.as_ref().is_some_and(has_metadata))
        .count() as u64;
    let assets_count = assets.len() as u64;
    let album_count = albums.len() as u64;
    MigrationPlanSummary {
        assets: assets_count,
        media_bytes: assets.iter().map(|asset| asset.byte_len).sum(),
        metadata_updates,
        album_creates: album_count,
        album_memberships: album_count,
        max_mutations: assets_count
            .saturating_add(metadata_updates)
            .saturating_add(album_count.saturating_mul(2)),
    }
}

const fn has_metadata(metadata: &NormalizedMetadata) -> bool {
    metadata.description.is_some() || metadata.taken_at_utc.is_some() || metadata.location.is_some()
}

fn server_valid(server: &MigrationServer) -> bool {
    lower_hex(&server.compatibility.identity_sha256, 64) && lower_hex(&server.origin_sha256, 64)
}

fn portable_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value.contains(['/', '\\'])
        && value != "."
        && value != ".."
        && !value.chars().any(char::is_control)
}

fn valid_unix_ms(value: i64) -> bool {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(value).saturating_mul(1_000_000)).is_ok()
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || (byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

/// Reason an immutable migration plan failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationPlanValidationError {
    /// The schema version is not supported.
    UnsupportedSchema(u32),
    /// A server, source or configuration binding is invalid or ambiguous.
    InvalidIdentity,
    /// An asset contains malformed or duplicate facts.
    InvalidAsset,
    /// Source asset IDs are not strictly sorted.
    AssetsNotStrictlySorted,
    /// A Live Photo image lacks its exact video dependency.
    InvalidLivePhotoDependency,
    /// An album or its membership is invalid.
    InvalidAlbum,
    /// Summary counters disagree with the plan body.
    SummaryMismatch,
}

impl Display for MigrationPlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported migration plan schema {version}")
            }
            Self::InvalidIdentity => formatter.write_str("invalid migration identity"),
            Self::InvalidAsset => formatter.write_str("invalid migration asset"),
            Self::AssetsNotStrictlySorted => {
                formatter.write_str("migration assets are not strictly sorted")
            }
            Self::InvalidLivePhotoDependency => {
                formatter.write_str("invalid migration Live Photo dependency")
            }
            Self::InvalidAlbum => formatter.write_str("invalid migration album"),
            Self::SummaryMismatch => formatter.write_str("migration summary mismatch"),
        }
    }
}

impl Error for MigrationPlanValidationError {}
