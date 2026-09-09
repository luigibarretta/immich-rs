use immich_rs_application::{
    ApplicationNoProgress, CancellationToken, SourceUploadPlanRequest, plan_apple_photos_upload,
};

use crate::args::UploadApplePhotosRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: UploadApplePhotosRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let client = network::read_client(
        &request.server,
        request.production_read,
        request.ca_certificate.as_deref(),
    )?;
    let plan = plan_apple_photos_upload(
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
    output::write_json(&plan, "Apple Photos upload plan")
}
