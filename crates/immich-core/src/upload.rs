use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    MediaKind, NormalizedMetadata, SourceDescriptor, SourceKind, UPLOAD_PLAN_SCHEMA_VERSION,
    UPLOAD_PLAN_SCHEMA_VERSION_V2,
};

/// Semantic Immich server version observed during read-only negotiation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
}

/// Server facts that bind an upload plan without exposing an endpoint or user ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerCompatibility {
    /// Exact version covered by the generated plan.
    pub version: ServerVersion,
    /// SHA-256 over the canonical origin and authenticated user identity.
    pub identity_sha256: String,
}

/// Optional XMP sidecar streamed with one asset upload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadSidecar {
    /// NFC-normalized source-relative path.
    pub relative_path: String,
    /// Observed sidecar length.
    pub byte_len: u64,
    /// SHA-256 observed during planning.
    pub content_sha256: String,
}

/// Execution role and dependency of one upload operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UploadRole {
    /// An independent image or video.
    Standalone,
    /// Motion-video half of a live photo.
    LivePhotoVideo {
        /// Stable pair identity.
        pair_id: String,
    },
    /// Still-image half uploaded after its motion video.
    LivePhotoImage {
        /// Stable pair identity.
        pair_id: String,
        /// Stable operation ID of the motion-video dependency.
        video_operation_id: String,
    },
}

/// One stable, source-verified server mutation in an upload plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadOperation {
    /// Stable operation ID inherited from the normalized plan.
    pub operation_id: String,
    /// NFC-normalized source-relative path.
    pub relative_path: String,
    /// Media class sent to Immich.
    pub media_kind: MediaKind,
    /// Expected media length.
    pub byte_len: u64,
    /// Expected SHA-256 media identity.
    pub content_sha256: String,
    /// Filesystem creation instant in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Filesystem modification instant in Unix milliseconds.
    pub modified_at_unix_ms: i64,
    /// Optional supported XMP sidecar.
    pub xmp_sidecar: Option<UploadSidecar>,
    /// Source-normalized metadata and album membership for import schema v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_metadata: Option<NormalizedMetadata>,
    /// Independent or live-photo execution role.
    pub role: UploadRole,
}

/// Deterministic counters for an immutable upload plan.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadPlanSummary {
    /// Planned media operations.
    pub operations: u64,
    /// Total planned media bytes, excluding sidecars.
    pub media_bytes: u64,
    /// Planned XMP sidecars.
    pub xmp_sidecars: u64,
    /// Complete image/video live-photo pairs.
    pub live_photo_pairs: u64,
    /// Asset metadata assignment requests in import schema v2.
    #[serde(default, skip_serializing_if = "is_default")]
    pub metadata_updates: u64,
    /// Maximum distinct album-create requests in import schema v2.
    #[serde(default, skip_serializing_if = "is_default")]
    pub album_creates: u64,
    /// Deterministic album-membership requests in import schema v2.
    #[serde(default, skip_serializing_if = "is_default")]
    pub album_memberships: u64,
    /// Maximum server mutations authorized by this immutable plan.
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_mutations: u64,
}

impl UploadPlanSummary {
    /// Derive deterministic counters from an operation set and plan schema.
    #[must_use]
    pub fn from_operations(schema_version: u32, operations: &[UploadOperation]) -> Self {
        expected_summary(schema_version, operations)
    }
}

/// Versioned immutable plan consumed by the Phase 2 apply capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadPlan {
    /// Upload plan schema version.
    pub schema_version: u32,
    /// Compact-serialization SHA-256 of the originating normalized plan.
    pub normalized_plan_sha256: String,
    /// Source identity inherited from the normalized plan.
    pub source: SourceDescriptor,
    /// SHA-256 of compatibility-affecting apply configuration.
    pub configuration_sha256: String,
    /// Authenticated server compatibility facts.
    pub server: ServerCompatibility,
    /// Operations sorted by portable relative path.
    pub operations: Vec<UploadOperation>,
    /// Deterministic plan counters.
    pub summary: UploadPlanSummary,
}

impl UploadPlan {
    /// Validate schema, identities, ordering, dependencies and counters.
    pub fn validate(&self) -> Result<(), UploadPlanValidationError> {
        if !matches!(
            self.schema_version,
            UPLOAD_PLAN_SCHEMA_VERSION | UPLOAD_PLAN_SCHEMA_VERSION_V2
        ) {
            return Err(UploadPlanValidationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if !is_sha256(&self.normalized_plan_sha256)
            || !is_sha256(&self.configuration_sha256)
            || !is_sha256(&self.source.fingerprint_sha256)
            || !is_sha256(&self.server.identity_sha256)
        {
            return Err(UploadPlanValidationError::InvalidIdentity);
        }
        let source_schema_matches = match self.schema_version {
            UPLOAD_PLAN_SCHEMA_VERSION => self.source.kind == SourceKind::Folder,
            UPLOAD_PLAN_SCHEMA_VERSION_V2 => matches!(
                self.source.kind,
                SourceKind::GoogleTakeout | SourceKind::ApplePhotos
            ),
            _ => false,
        };
        if !source_schema_matches {
            return Err(UploadPlanValidationError::InvalidSchemaSource);
        }
        if self.schema_version == UPLOAD_PLAN_SCHEMA_VERSION
            && self
                .operations
                .iter()
                .any(|operation| operation.normalized_metadata.is_some())
        {
            return Err(UploadPlanValidationError::InvalidMetadata);
        }
        let mut ids = BTreeSet::new();
        let mut roles = BTreeMap::new();
        let mut previous_path: Option<&str> = None;
        for operation in &self.operations {
            validate_operation(operation, previous_path, &mut ids)?;
            roles.insert(operation.operation_id.as_str(), &operation.role);
            previous_path = Some(operation.relative_path.as_str());
        }
        validate_dependencies(&self.operations, &roles)?;
        if expected_summary(self.schema_version, &self.operations) != self.summary {
            return Err(UploadPlanValidationError::SummaryMismatch);
        }
        Ok(())
    }
}

fn validate_operation<'a>(
    operation: &'a UploadOperation,
    previous_path: Option<&str>,
    ids: &mut BTreeSet<&'a str>,
) -> Result<(), UploadPlanValidationError> {
    if operation.relative_path.is_empty()
        || !is_sha256(&operation.operation_id)
        || !is_sha256(&operation.content_sha256)
        || operation.byte_len == 0
    {
        return Err(UploadPlanValidationError::InvalidOperation);
    }
    if previous_path.is_some_and(|path| path >= operation.relative_path.as_str()) {
        return Err(UploadPlanValidationError::OperationsNotStrictlySorted);
    }
    if !ids.insert(operation.operation_id.as_str()) {
        return Err(UploadPlanValidationError::DuplicateOperationId);
    }
    if operation.xmp_sidecar.as_ref().is_some_and(|sidecar| {
        sidecar.relative_path.is_empty()
            || sidecar.byte_len == 0
            || !is_sha256(&sidecar.content_sha256)
    }) {
        return Err(UploadPlanValidationError::InvalidSidecar);
    }
    if operation
        .normalized_metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.is_valid())
    {
        return Err(UploadPlanValidationError::InvalidMetadata);
    }
    Ok(())
}

fn validate_dependencies(
    operations: &[UploadOperation],
    roles: &BTreeMap<&str, &UploadRole>,
) -> Result<(), UploadPlanValidationError> {
    for operation in operations {
        if let UploadRole::LivePhotoImage {
            pair_id,
            video_operation_id,
        } = &operation.role
        {
            match roles.get(video_operation_id.as_str()) {
                Some(UploadRole::LivePhotoVideo {
                    pair_id: video_pair,
                }) if video_pair == pair_id => {}
                _ => return Err(UploadPlanValidationError::InvalidLivePhotoDependency),
            }
        }
    }
    Ok(())
}

fn expected_summary(schema_version: u32, operations: &[UploadOperation]) -> UploadPlanSummary {
    let albums = operations
        .iter()
        .filter_map(|operation| operation.normalized_metadata.as_ref())
        .flat_map(|metadata| metadata.albums.iter().cloned())
        .collect::<BTreeSet<_>>();
    let metadata_updates = operations
        .iter()
        .filter(|operation| {
            operation
                .normalized_metadata
                .as_ref()
                .is_some_and(|metadata| {
                    metadata.description.is_some()
                        || metadata.taken_at_utc.is_some()
                        || metadata.location.is_some()
                })
        })
        .count() as u64;
    let operations_count = operations.len() as u64;
    let import = schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2;
    let metadata_updates = if import { metadata_updates } else { 0 };
    let album_count = if import { albums.len() as u64 } else { 0 };
    UploadPlanSummary {
        operations: operations_count,
        media_bytes: operations.iter().map(|operation| operation.byte_len).sum(),
        xmp_sidecars: operations
            .iter()
            .filter(|operation| operation.xmp_sidecar.is_some())
            .count() as u64,
        live_photo_pairs: operations
            .iter()
            .filter(|operation| matches!(operation.role, UploadRole::LivePhotoImage { .. }))
            .count() as u64,
        metadata_updates,
        album_creates: album_count,
        album_memberships: album_count,
        max_mutations: if import {
            operations_count
                .saturating_add(metadata_updates)
                .saturating_add(album_count.saturating_mul(2))
        } else {
            0
        },
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Reason an immutable upload plan failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadPlanValidationError {
    /// The plan schema is not supported.
    UnsupportedSchema(u32),
    /// A plan, source, configuration or server digest is malformed.
    InvalidIdentity,
    /// An operation has invalid source facts.
    InvalidOperation,
    /// Operation paths are not strictly ascending.
    OperationsNotStrictlySorted,
    /// Stable operation IDs are not unique.
    DuplicateOperationId,
    /// An XMP sidecar has malformed source facts.
    InvalidSidecar,
    /// Normalized metadata is malformed.
    InvalidMetadata,
    /// Upload schema does not match its source adapter.
    InvalidSchemaSource,
    /// A live-photo image lacks its matching video operation.
    InvalidLivePhotoDependency,
    /// Summary counters disagree with operations.
    SummaryMismatch,
}

impl Display for UploadPlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported upload plan schema {version}")
            }
            Self::InvalidIdentity => formatter.write_str("invalid upload plan identity"),
            Self::InvalidOperation => formatter.write_str("invalid upload operation"),
            Self::OperationsNotStrictlySorted => {
                formatter.write_str("upload operations are not strictly sorted")
            }
            Self::DuplicateOperationId => formatter.write_str("duplicate upload operation ID"),
            Self::InvalidSidecar => formatter.write_str("invalid upload sidecar"),
            Self::InvalidMetadata => formatter.write_str("invalid upload metadata"),
            Self::InvalidSchemaSource => {
                formatter.write_str("upload schema does not match source adapter")
            }
            Self::InvalidLivePhotoDependency => {
                formatter.write_str("invalid live-photo upload dependency")
            }
            Self::SummaryMismatch => formatter.write_str("upload plan summary mismatch"),
        }
    }
}

impl Error for UploadPlanValidationError {}
