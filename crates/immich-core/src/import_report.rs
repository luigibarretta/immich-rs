use serde::{Deserialize, Serialize};

use crate::{IMPORT_APPLY_REPORT_SCHEMA_VERSION, UploadPlanSummary};

/// Privacy-aware aggregate outcome of one source-aware import invocation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportApplyReport {
    /// Import report schema version.
    pub schema_version: u32,
    /// Whether no mutation capability was constructed.
    pub dry_run: bool,
    /// Exact counters authorized by the immutable plan.
    pub planned: UploadPlanSummary,
    /// Asset uploads a dry-run would attempt.
    pub would_upload: u64,
    /// Metadata assignments a dry-run would attempt.
    pub would_update_metadata: u64,
    /// Album creates a dry-run might require.
    pub would_create_albums: u64,
    /// Album membership requests a dry-run would attempt.
    pub would_add_album_memberships: u64,
    /// Assets created by this invocation.
    pub created: u64,
    /// Asset uploads converged through duplicate detection.
    pub duplicate: u64,
    /// Metadata assignments completed.
    pub metadata_updated: u64,
    /// Albums created by this invocation.
    pub albums_created: u64,
    /// Exact-name albums reused by this invocation.
    pub albums_reused: u64,
    /// Album membership requests completed.
    pub album_memberships_updated: u64,
    /// Durable effects skipped during resume.
    pub resumed_effects: u64,
    /// Transient requests repeated within budget.
    pub retried: u64,
    /// Effects with a definite failure.
    pub failed: u64,
    /// Effects whose durable outcome is not yet known.
    pub indeterminate: u64,
    /// Whether cancellation stopped scheduling new effects.
    pub cancelled: bool,
}

impl ImportApplyReport {
    /// Create an offline report for one fully verified immutable plan.
    #[must_use]
    pub const fn dry_run(planned: UploadPlanSummary) -> Self {
        Self {
            schema_version: IMPORT_APPLY_REPORT_SCHEMA_VERSION,
            dry_run: true,
            planned,
            would_upload: planned.operations,
            would_update_metadata: planned.metadata_updates,
            would_create_albums: planned.album_creates,
            would_add_album_memberships: planned.album_memberships,
            created: 0,
            duplicate: 0,
            metadata_updated: 0,
            albums_created: 0,
            albums_reused: 0,
            album_memberships_updated: 0,
            resumed_effects: 0,
            retried: 0,
            failed: 0,
            indeterminate: 0,
            cancelled: false,
        }
    }
}
