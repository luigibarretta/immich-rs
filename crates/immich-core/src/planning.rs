use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    NORMALIZED_PLAN_SCHEMA_VERSION, NORMALIZED_PLAN_SCHEMA_VERSION_V2,
    NORMALIZED_PLAN_SCHEMA_VERSION_V3, NormalizedMetadata,
};

/// Kind of input adapter that produced a plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A recursively enumerated filesystem folder.
    Folder,
    /// A decompressed or split-archive Google Takeout export.
    GoogleTakeout,
    /// An Apple Photos or iCloud Photos export.
    ApplePhotos,
}

/// Explicit Unicode normalization policy applied to portable relative paths.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnicodeNormalization {
    /// Unicode paths are normalized to NFC.
    Nfc,
}

/// Stable, privacy-aware description of a scanned source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDescriptor {
    /// Adapter family.
    pub kind: SourceKind,
    /// Caller-provided non-secret label, never an absolute filesystem path.
    pub label: String,
    /// SHA-256 over sorted portable paths, content identities and associations.
    pub fingerprint_sha256: String,
    /// Whether case-sensitive portable paths were used during discovery.
    pub case_sensitive: bool,
    /// Unicode normalization applied to every path in the plan.
    pub unicode_normalization: UnicodeNormalization,
}

/// Media classification used by the first folder compatibility matrix.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// A supported still-image extension.
    Image,
    /// A supported video extension.
    Video,
}

/// Supported metadata sidecar family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataKind {
    /// JSON metadata sidecar.
    Json,
    /// XMP metadata sidecar.
    Xmp,
}

/// Role of an asset in a live-photo pair.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LivePhotoRole {
    /// The still-image component.
    Image,
    /// The motion-video component.
    Video,
}

/// Explainable evidence used to make a planning decision.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleEvidence {
    /// Stable rule identifier.
    pub rule_id: String,
    /// Non-secret machine-readable outcome.
    pub outcome: String,
}

/// A sidecar that may contribute metadata to an asset.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataCandidate {
    /// NFC-normalized path relative to the source root.
    pub relative_path: String,
    /// Sidecar family.
    pub kind: MetadataKind,
    /// Rule that associated the sidecar with the asset.
    pub rule_id: String,
}

/// Stable live-photo membership without an execution action.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LivePhotoMember {
    /// Stable pair identifier derived from both relative paths.
    pub pair_id: String,
    /// Asset role within the pair.
    pub role: LivePhotoRole,
}

/// One source asset described without any server mutation capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAsset {
    /// Stable operation identifier derived from source facts.
    pub operation_id: String,
    /// NFC-normalized path relative to the source root.
    pub relative_path: String,
    /// Media family.
    pub media_kind: MediaKind,
    /// Number of bytes observed during the streaming read.
    pub byte_len: u64,
    /// SHA-256 computed while streaming the file through a bounded buffer.
    pub content_sha256: String,
    /// Deterministically associated metadata candidates.
    pub metadata: Vec<MetadataCandidate>,
    /// Resolved source-neutral metadata introduced by normalized-plan-v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_metadata: Option<NormalizedMetadata>,
    /// Pair identifier and role when this asset is part of a live photo.
    pub live_photo: Option<LivePhotoMember>,
    /// Rules that explain why this asset is in the plan.
    pub evidence: Vec<RuleEvidence>,
}

/// Explainable warning or error recorded in a normalized plan.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDiagnostic {
    /// Stable rule identifier.
    pub rule_id: String,
    /// Stable diagnostic class.
    pub code: String,
    /// Bounded portable paths involved in the diagnostic.
    pub paths: Vec<String>,
}

/// Read-only scan counters included in the plan for reproducibility.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanSummary {
    /// Regular media assets described by the plan.
    pub assets: u64,
    /// Metadata sidecars associated with assets.
    pub sidecars: u64,
    /// Filesystem bytes read while calculating content identities.
    pub bytes_read: u64,
}

/// Versioned immutable output of a read-only source scan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPlan {
    /// Normalized plan schema version.
    pub schema_version: u32,
    /// Source identity and normalization choices.
    pub source: SourceDescriptor,
    /// Assets sorted by portable relative path.
    pub assets: Vec<CandidateAsset>,
    /// Non-fatal findings sorted by rule and path.
    pub warnings: Vec<PlanDiagnostic>,
    /// Findings that prevent safe apply, sorted by rule and path.
    pub errors: Vec<PlanDiagnostic>,
    /// Deterministic scan counters.
    pub summary: PlanSummary,
}

impl NormalizedPlan {
    /// Validate schema, ordering and stable identifier invariants.
    pub fn validate(&self) -> Result<(), PlanValidationError> {
        if !matches!(
            self.schema_version,
            NORMALIZED_PLAN_SCHEMA_VERSION
                | NORMALIZED_PLAN_SCHEMA_VERSION_V2
                | NORMALIZED_PLAN_SCHEMA_VERSION_V3
        ) {
            return Err(PlanValidationError::UnsupportedSchema(self.schema_version));
        }
        if self.schema_version == NORMALIZED_PLAN_SCHEMA_VERSION_V2
            && self.source.kind != SourceKind::GoogleTakeout
        {
            return Err(PlanValidationError::InvalidSchemaSource);
        }
        if self.schema_version == NORMALIZED_PLAN_SCHEMA_VERSION_V3
            && self.source.kind != SourceKind::ApplePhotos
        {
            return Err(PlanValidationError::InvalidSchemaSource);
        }
        if self.source.kind == SourceKind::ApplePhotos
            && self.schema_version != NORMALIZED_PLAN_SCHEMA_VERSION_V3
        {
            return Err(PlanValidationError::InvalidSchemaSource);
        }
        if self.source.label.is_empty() || self.source.fingerprint_sha256.len() != 64 {
            return Err(PlanValidationError::InvalidSourceDescriptor);
        }
        let mut previous_path: Option<&str> = None;
        let mut operation_ids = BTreeSet::new();
        for asset in &self.assets {
            if asset.relative_path.is_empty()
                || asset.operation_id.len() != 64
                || asset.content_sha256.len() != 64
            {
                return Err(PlanValidationError::InvalidAssetIdentity);
            }
            if previous_path.is_some_and(|path| path >= asset.relative_path.as_str()) {
                return Err(PlanValidationError::AssetsNotStrictlySorted);
            }
            if !operation_ids.insert(asset.operation_id.as_str()) {
                return Err(PlanValidationError::DuplicateOperationId);
            }
            if asset
                .evidence
                .iter()
                .any(|evidence| evidence.rule_id.is_empty())
                || asset
                    .metadata
                    .iter()
                    .any(|metadata| metadata.rule_id.is_empty())
            {
                return Err(PlanValidationError::MissingRuleId);
            }
            if (self.schema_version == NORMALIZED_PLAN_SCHEMA_VERSION
                && asset.normalized_metadata.is_some())
                || asset
                    .normalized_metadata
                    .as_ref()
                    .is_some_and(|metadata| !metadata.is_valid())
            {
                return Err(PlanValidationError::InvalidNormalizedMetadata);
            }
            previous_path = Some(asset.relative_path.as_str());
        }
        let sidecar_count = self
            .assets
            .iter()
            .map(|asset| asset.metadata.len() as u64)
            .sum::<u64>();
        if self.summary.assets != self.assets.len() as u64
            || self.summary.sidecars != sidecar_count
            || !self.warnings.windows(2).all(|pair| pair[0] <= pair[1])
            || !self.errors.windows(2).all(|pair| pair[0] <= pair[1])
        {
            return Err(PlanValidationError::SummaryMismatch);
        }
        Ok(())
    }
}

/// Reason a normalized plan failed its stable invariants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanValidationError {
    /// The plan schema is not supported by this binary.
    UnsupportedSchema(u32),
    /// The schema version is not valid for the selected source adapter.
    InvalidSchemaSource,
    /// The source descriptor is incomplete or malformed.
    InvalidSourceDescriptor,
    /// An asset has an empty path or malformed digest.
    InvalidAssetIdentity,
    /// Asset paths are not unique and strictly ascending.
    AssetsNotStrictlySorted,
    /// Two adjacent assets have the same stable operation identifier.
    DuplicateOperationId,
    /// Explainable evidence omitted its stable rule identifier.
    MissingRuleId,
    /// Resolved metadata is malformed or unavailable in this schema version.
    InvalidNormalizedMetadata,
    /// Summary counters disagree with plan contents.
    SummaryMismatch,
}

impl Display for PlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported normalized plan schema {version}")
            }
            Self::InvalidSchemaSource => {
                formatter.write_str("normalized plan schema is invalid for the source adapter")
            }
            Self::InvalidSourceDescriptor => formatter.write_str("invalid source descriptor"),
            Self::InvalidAssetIdentity => formatter.write_str("invalid asset identity"),
            Self::AssetsNotStrictlySorted => {
                formatter.write_str("assets are not strictly sorted by relative path")
            }
            Self::DuplicateOperationId => formatter.write_str("duplicate operation identifier"),
            Self::MissingRuleId => formatter.write_str("missing rule identifier"),
            Self::InvalidNormalizedMetadata => {
                formatter.write_str("invalid normalized asset metadata")
            }
            Self::SummaryMismatch => formatter.write_str("plan summary does not match assets"),
        }
    }
}

impl Error for PlanValidationError {}
