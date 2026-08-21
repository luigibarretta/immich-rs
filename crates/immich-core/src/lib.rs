#![forbid(unsafe_code)]
//! Source-neutral domain and execution contracts.

mod cancellation;
mod metadata;
mod planning;
mod progress;
pub mod rule_id;
mod upload;

pub use cancellation::{Cancellation, CancellationToken, NeverCancel};
pub use metadata::{GeoCoordinates, NormalizedMetadata};
pub use planning::{
    CandidateAsset, LivePhotoMember, LivePhotoRole, MediaKind, MetadataCandidate, MetadataKind,
    NormalizedPlan, PlanDiagnostic, PlanSummary, PlanValidationError, RuleEvidence,
    SourceDescriptor, SourceKind, UnicodeNormalization,
};
pub use progress::{ProgressEvent, ProgressStage};
pub use upload::{
    ApplyReport, ServerCompatibility, ServerVersion, UploadOperation, UploadPlan,
    UploadPlanSummary, UploadPlanValidationError, UploadRole, UploadSidecar,
};

/// Schema version for synthetic fixture manifests.
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;
/// Schema version for normalized read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for complete Google Takeout read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION_V2: u32 = 2;
/// Schema version for Apple Photos export read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION_V3: u32 = 3;
/// Schema version for progress events.
pub const PROGRESS_EVENT_SCHEMA_VERSION: u32 = 1;
/// Schema version for immutable upload plans.
pub const UPLOAD_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for privacy-aware apply reports.
pub const APPLY_REPORT_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
mod tests;
