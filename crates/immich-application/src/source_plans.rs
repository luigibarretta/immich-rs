use std::path::PathBuf;

use immich_rs_core::{Cancellation, NormalizedPlan};
use immich_rs_sources::{
    ApplePhotosScanConfig, PicasaScanConfig, ScanError, TakeoutScanConfig,
    scan_apple_photos_inputs, scan_google_takeout_inputs, scan_picasa_inputs,
};

use crate::progress::{ApplicationProgressObserver, ScanProgressAdapter};

/// Complete input for one deterministic source-only plan.
#[derive(Clone, Debug)]
pub struct SourcePlanRequest<Config> {
    /// One directory or a bounded set of independent ZIP inputs.
    pub inputs: Vec<PathBuf>,
    /// Source-neutral label serialized into the normalized plan.
    pub label: String,
    /// Explicit adapter and resource configuration.
    pub config: Config,
}

/// Produce the existing Google Takeout normalized plan.
pub fn plan_google_takeout(
    request: &SourcePlanRequest<TakeoutScanConfig>,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    let mut adapter = ScanProgressAdapter::new(observer);
    scan_google_takeout_inputs(
        &request.inputs,
        &request.label,
        &request.config,
        cancellation,
        &mut adapter,
    )
}

/// Produce the existing Apple Photos normalized plan.
pub fn plan_apple_photos(
    request: &SourcePlanRequest<ApplePhotosScanConfig>,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    let mut adapter = ScanProgressAdapter::new(observer);
    scan_apple_photos_inputs(
        &request.inputs,
        &request.label,
        &request.config,
        cancellation,
        &mut adapter,
    )
}

/// Produce the existing Picasa normalized plan.
pub fn plan_picasa(
    request: &SourcePlanRequest<PicasaScanConfig>,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    let mut adapter = ScanProgressAdapter::new(observer);
    scan_picasa_inputs(
        &request.inputs,
        &request.label,
        &request.config,
        cancellation,
        &mut adapter,
    )
}
