use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{APPLY_REPORT_SCHEMA_VERSION, MediaKind, SourceDescriptor, UPLOAD_PLAN_SCHEMA_VERSION};

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
        if self.schema_version != UPLOAD_PLAN_SCHEMA_VERSION {
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
        let mut ids = BTreeSet::new();
        let mut roles = BTreeMap::new();
        let mut previous_path: Option<&str> = None;
        for operation in &self.operations {
            validate_operation(operation, previous_path, &mut ids)?;
            roles.insert(operation.operation_id.as_str(), &operation.role);
            previous_path = Some(operation.relative_path.as_str());
        }
        validate_dependencies(&self.operations, &roles)?;
        if expected_summary(&self.operations) != self.summary {
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

fn expected_summary(operations: &[UploadOperation]) -> UploadPlanSummary {
    UploadPlanSummary {
        operations: operations.len() as u64,
        media_bytes: operations.iter().map(|operation| operation.byte_len).sum(),
        xmp_sidecars: operations
            .iter()
            .filter(|operation| operation.xmp_sidecar.is_some())
            .count() as u64,
        live_photo_pairs: operations
            .iter()
            .filter(|operation| matches!(operation.role, UploadRole::LivePhotoImage { .. }))
            .count() as u64,
    }
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
            Self::InvalidLivePhotoDependency => {
                formatter.write_str("invalid live-photo upload dependency")
            }
            Self::SummaryMismatch => formatter.write_str("upload plan summary mismatch"),
        }
    }
}

impl Error for UploadPlanValidationError {}

/// Privacy-aware aggregate outcome of one apply invocation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyReport {
    /// Apply report schema version.
    pub schema_version: u32,
    /// Whether no mutation capability was constructed.
    pub dry_run: bool,
    /// Operations in the immutable plan.
    pub planned: u64,
    /// Operations a dry-run would attempt.
    pub would_upload: u64,
    /// Assets created by this invocation.
    pub created: u64,
    /// Operations converged through server duplicate detection.
    pub duplicate: u64,
    /// Completed journal operations skipped on resume.
    pub resumed: u64,
    /// Transient attempts repeated within budget.
    pub retried: u64,
    /// Operations with a definite failure.
    pub failed: u64,
    /// Operations whose durable outcome is not yet known.
    pub indeterminate: u64,
    /// Whether cancellation stopped scheduling new operations.
    pub cancelled: bool,
}

impl ApplyReport {
    /// Create an empty report for one validated plan and mode.
    #[must_use]
    pub const fn new(planned: u64, dry_run: bool) -> Self {
        Self {
            schema_version: APPLY_REPORT_SCHEMA_VERSION,
            dry_run,
            planned,
            would_upload: 0,
            created: 0,
            duplicate: 0,
            resumed: 0,
            retried: 0,
            failed: 0,
            indeterminate: 0,
            cancelled: false,
        }
    }
}
