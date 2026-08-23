use immich_rs_core::CancellationToken;
use immich_rs_executor::create_takeout_upload_plan;
use immich_rs_sources::{NoProgress, scan_google_takeout_inputs_resolved};

use crate::args::UploadTakeoutRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: UploadTakeoutRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let client = network::read_client(
        &request.server,
        request.production_read,
        request.ca_certificate.as_deref(),
    )?;
    let negotiated = client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let resolved = scan_google_takeout_inputs_resolved(
        &request.inputs,
        &request.label,
        &request.config.source,
        &cancellation,
        &mut NoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    let plan = create_takeout_upload_plan(
        &resolved,
        negotiated.compatibility().clone(),
        &request.config,
    )
    .map_err(CliFailure::from_executor)?;
    output::write_json(&plan, "Google Takeout upload plan")
}
