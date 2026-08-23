use immich_rs_core::{CancellationToken, UPLOAD_PLAN_SCHEMA_VERSION};
use immich_rs_executor::dry_run_upload;

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    if plan.schema_version != UPLOAD_PLAN_SCHEMA_VERSION {
        return Err(CliFailure::usage(
            "apply upload does not yet support source-aware import plans",
        ));
    }
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
