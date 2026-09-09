use immich_rs_application::{
    ApplicationNoProgress, CancellationToken, SourcePlanRequest, plan_google_takeout,
};

use crate::args::TakeoutRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &TakeoutRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let plan = plan_google_takeout(
        &SourcePlanRequest {
            inputs: request.inputs.clone(),
            label: request.label.clone(),
            config: request.config.clone(),
        },
        &cancellation,
        &mut ApplicationNoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    output::write_json(&plan, "normalized Google Takeout plan")
}
