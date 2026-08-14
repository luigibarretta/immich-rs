#![forbid(unsafe_code)]
//! Source-neutral domain types for read-only discovery and planning.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// Schema version for synthetic fixture manifests.
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;
/// Schema version for normalized read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for progress events.
pub const PROGRESS_EVENT_SCHEMA_VERSION: u32 = 1;

/// Stable rule identifiers shared by scanners, plans and diagnostics.
pub mod rule_id {
    /// A regular media file was accepted after a bounded streaming read.
    pub const REGULAR_MEDIA: &str = "FS_REGULAR_MEDIA_V1";
    /// A sidecar was associated by an exact relative filename.
    pub const SIDECAR_EXACT_NAME: &str = "META_SIDECAR_EXACT_NAME_V1";
    /// A sidecar was associated by an unambiguous basename.
    pub const SIDECAR_BASENAME: &str = "META_SIDECAR_BASENAME_V1";
    /// A still image and video were associated as a live-photo pair.
    pub const LIVE_PHOTO_BASENAME: &str = "PAIR_LIVE_PHOTO_BASENAME_V1";
    /// A symbolic link was deliberately not followed.
    pub const SYMLINK_SKIPPED: &str = "FS_SYMLINK_SKIPPED_V1";
    /// A path is not valid Unicode and cannot enter the portable plan.
    pub const NON_UNICODE_PATH: &str = "FS_NON_UNICODE_PATH_V1";
    /// Two portable paths differ only by case.
    pub const CASE_COLLISION: &str = "FS_CASE_COLLISION_V1";
    /// Multiple assets share a basename in distinct directories.
    pub const DUPLICATE_BASENAME: &str = "FS_DUPLICATE_BASENAME_V1";
    /// A sidecar could not be associated without ambiguity.
    pub const AMBIGUOUS_SIDECAR: &str = "META_AMBIGUOUS_SIDECAR_V1";
    /// A file could not be opened or read.
    pub const UNREADABLE_FILE: &str = "FS_UNREADABLE_FILE_V1";
    /// File identity changed while content was being read.
    pub const SOURCE_CHANGED: &str = "FS_SOURCE_CHANGED_V1";
    /// A non-file filesystem entry was deliberately ignored.
    pub const SPECIAL_FILE_SKIPPED: &str = "FS_SPECIAL_FILE_SKIPPED_V1";
}

/// Kind of input adapter that produced a plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A recursively enumerated filesystem folder.
    Folder,
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
    /// Pair identifier and role when this asset is part of a live photo.
    pub live_photo: Option<LivePhotoMember>,
    /// Rules that explain why this asset is in the plan.
    pub evidence: Vec<RuleEvidence>,
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
        if self.schema_version != NORMALIZED_PLAN_SCHEMA_VERSION {
            return Err(PlanValidationError::UnsupportedSchema(self.schema_version));
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
    /// Summary counters disagree with plan contents.
    SummaryMismatch,
}

impl Display for PlanValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported normalized plan schema {version}")
            }
            Self::InvalidSourceDescriptor => formatter.write_str("invalid source descriptor"),
            Self::InvalidAssetIdentity => formatter.write_str("invalid asset identity"),
            Self::AssetsNotStrictlySorted => {
                formatter.write_str("assets are not strictly sorted by relative path")
            }
            Self::DuplicateOperationId => formatter.write_str("duplicate operation identifier"),
            Self::MissingRuleId => formatter.write_str("missing rule identifier"),
            Self::SummaryMismatch => formatter.write_str("plan summary does not match assets"),
        }
    }
}

impl Error for PlanValidationError {}

/// Pipeline stage represented by a progress event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    /// Filesystem entry discovery.
    Discovery,
    /// Bounded content hashing.
    ContentIdentity,
    /// Deterministic metadata and live-photo matching.
    Reconciliation,
    /// Immutable plan finalization.
    Complete,
}

/// Stable, low-cardinality progress event that omits media paths and metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressEvent {
    /// Progress event schema version.
    pub schema_version: u32,
    /// Monotonic event sequence within one scan.
    pub sequence: u64,
    /// Current read-only stage.
    pub stage: ProgressStage,
    /// Assets observed so far.
    pub assets_observed: u64,
    /// Media bytes streamed so far.
    pub bytes_read: u64,
}

/// Cooperative cancellation contract for bounded pipeline stages.
pub trait Cancellation: Send + Sync {
    /// Return `true` when the caller requests a clean stop.
    fn is_cancelled(&self) -> bool;
}

/// Cloneable cancellation token backed by one atomic flag.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Cancellation for CancellationToken {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Cancellation implementation for callers that never cancel.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cancellation, CancellationToken, CandidateAsset, MediaKind, NORMALIZED_PLAN_SCHEMA_VERSION,
        NeverCancel, NormalizedPlan, PlanSummary, SourceDescriptor, SourceKind,
        UnicodeNormalization,
    };

    fn valid_plan() -> NormalizedPlan {
        NormalizedPlan {
            schema_version: NORMALIZED_PLAN_SCHEMA_VERSION,
            source: SourceDescriptor {
                kind: SourceKind::Folder,
                label: "synthetic-basic".to_owned(),
                fingerprint_sha256: "a".repeat(64),
                case_sensitive: true,
                unicode_normalization: UnicodeNormalization::Nfc,
            },
            assets: vec![CandidateAsset {
                operation_id: "b".repeat(64),
                relative_path: "image.jpg".to_owned(),
                media_kind: MediaKind::Image,
                byte_len: 4,
                content_sha256: "c".repeat(64),
                metadata: Vec::new(),
                live_photo: None,
                evidence: Vec::new(),
            }],
            warnings: Vec::new(),
            errors: Vec::new(),
            summary: PlanSummary {
                assets: 1,
                sidecars: 0,
                bytes_read: 4,
            },
        }
    }

    #[test]
    fn normalized_plan_round_trips_without_losing_schema_facts()
    -> Result<(), Box<dyn std::error::Error>> {
        let plan = valid_plan();
        let encoded = serde_json::to_string(&plan)?;
        let decoded: NormalizedPlan = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, plan);
        decoded.validate()?;
        Ok(())
    }

    #[test]
    fn normalized_plan_rejects_unsorted_assets() {
        let mut plan = valid_plan();
        let mut second = plan.assets[0].clone();
        second.relative_path = "another.jpg".to_owned();
        second.operation_id = "d".repeat(64);
        plan.assets.push(second);
        plan.summary.assets = 2;
        assert!(plan.validate().is_err());
    }

    #[test]
    fn cancellation_is_explicit_and_clone_safe() {
        let token = CancellationToken::default();
        let observer = token.clone();
        assert!(!observer.is_cancelled());
        token.cancel();
        assert!(observer.is_cancelled());
        assert!(!NeverCancel.is_cancelled());
    }
}
