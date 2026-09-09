use immich_rs_application::{
    ApplicationNoProgress, CancellationToken, FolderPlanRequest, plan_folder,
};

use crate::args::FolderRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &FolderRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let plan = plan_folder(
        &FolderPlanRequest {
            root: request.root.clone(),
            label: request.label.clone(),
            config: request.config.clone(),
        },
        &cancellation,
        &mut ApplicationNoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    output::write_json(&plan, "normalized plan")
}
