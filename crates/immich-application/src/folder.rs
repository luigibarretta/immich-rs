use std::path::PathBuf;

use immich_rs_core::{Cancellation, NormalizedPlan};
use immich_rs_sources::{FolderScanConfig, ScanError, scan_folder};

use crate::progress::{ApplicationProgressObserver, ScanProgressAdapter};

/// Complete input for one deterministic, read-only folder plan.
#[derive(Clone, Debug)]
pub struct FolderPlanRequest {
    /// Operator-configured source root. Frontends retain ownership of access policy.
    pub root: PathBuf,
    /// Source-neutral label serialized into the normalized plan.
    pub label: String,
    /// Explicit bounded scanner configuration.
    pub config: FolderScanConfig,
}

/// Produce the existing normalized folder plan through a frontend-neutral facade.
pub fn plan_folder(
    request: &FolderPlanRequest,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    let mut adapter = ScanProgressAdapter::new(observer);
    scan_folder(
        &request.root,
        &request.label,
        &request.config,
        cancellation,
        &mut adapter,
    )
}
