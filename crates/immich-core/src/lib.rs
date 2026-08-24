#![forbid(unsafe_code)]
//! Source-neutral domain and execution contracts.

mod apply_report;
mod archive;
mod cancellation;
mod import_report;
mod metadata;
mod migration;
mod planning;
mod production;
mod progress;
pub mod rule_id;
mod upload;

pub use apply_report::ApplyReport;
pub use archive::{
    ArchiveApplyReport, ArchiveAsset, ArchiveManifest, ArchiveManifestSummary,
    ArchiveManifestValidationError,
};
pub use cancellation::{Cancellation, CancellationToken, NeverCancel};
pub use import_report::ImportApplyReport;
pub use metadata::{GeoCoordinates, NormalizedMetadata};
pub use migration::{
    MigrationAlbum, MigrationAsset, MigrationPlan, MigrationPlanSummary,
    MigrationPlanValidationError, MigrationServer,
};
pub use planning::{
    CandidateAsset, LivePhotoMember, LivePhotoRole, MediaKind, MetadataCandidate, MetadataKind,
    NormalizedPlan, PlanDiagnostic, PlanSummary, PlanValidationError, RuleEvidence,
    SourceDescriptor, SourceKind, UnicodeNormalization,
};
pub use production::{ProductionConfirmationError, ProductionWriteConfirmation};
pub use progress::{ProgressEvent, ProgressStage};
pub use upload::{
    ServerCompatibility, ServerVersion, UploadOperation, UploadPlan, UploadPlanSummary,
    UploadPlanValidationError, UploadRole, UploadSidecar,
};

/// Schema version for synthetic fixture manifests.
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;
/// Schema version for normalized read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for complete Google Takeout read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION_V2: u32 = 2;
/// Schema version for Apple Photos export read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION_V3: u32 = 3;
/// Schema version for Picasa export read-only plans.
pub const NORMALIZED_PLAN_SCHEMA_VERSION_V4: u32 = 4;
/// Schema version for progress events.
pub const PROGRESS_EVENT_SCHEMA_VERSION: u32 = 1;
/// Schema version for immutable upload plans.
pub const UPLOAD_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for source-aware imports with normalized metadata and albums.
pub const UPLOAD_PLAN_SCHEMA_VERSION_V2: u32 = 2;
/// Schema version for privacy-aware apply reports.
pub const APPLY_REPORT_SCHEMA_VERSION: u32 = 1;
/// Schema version for source-aware import apply reports.
pub const IMPORT_APPLY_REPORT_SCHEMA_VERSION: u32 = 1;
/// Schema version for immutable read-only Immich archive manifests.
pub const ARCHIVE_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Schema version for privacy-aware local archive apply reports.
pub const ARCHIVE_APPLY_REPORT_SCHEMA_VERSION: u32 = 1;
/// Schema version for immutable Immich-to-Immich migration plans.
pub const MIGRATION_PLAN_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod tests;
