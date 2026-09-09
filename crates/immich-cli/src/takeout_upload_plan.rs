use immich_rs_application::{
    ApplicationNoProgress, CancellationToken, SourceUploadPlanRequest, plan_google_takeout_upload,
};

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
    let plan = plan_google_takeout_upload(
        &SourceUploadPlanRequest {
            inputs: request.inputs,
            label: request.label,
            config: request.config,
        },
        &client,
        &cancellation,
        &mut ApplicationNoProgress,
    )
    .await
    .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&plan, "Google Takeout upload plan")
}
