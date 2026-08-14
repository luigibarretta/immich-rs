use immich_rs_core::CancellationToken;
use immich_rs_executor::dry_run_upload;

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let report = dry_run_upload(
        &plan,
        &request.source,
        &request.checkpoint,
        &request.config,
        &cancellation,
    )
    .map_err(CliFailure::from_executor)?;
    output::write_json(&report, "dry-run report")
}
