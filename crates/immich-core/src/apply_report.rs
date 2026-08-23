use serde::{Deserialize, Serialize};

use crate::APPLY_REPORT_SCHEMA_VERSION;

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
