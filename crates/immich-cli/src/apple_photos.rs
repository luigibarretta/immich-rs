use immich_rs_core::CancellationToken;
use immich_rs_sources::{NoProgress, scan_apple_photos_inputs};

use crate::args::ApplePhotosRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &ApplePhotosRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let plan = scan_apple_photos_inputs(
        &request.inputs,
        &request.label,
        &request.config,
        &cancellation,
        &mut NoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    output::write_json(&plan, "normalized Apple Photos plan")
}
