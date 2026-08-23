use immich_rs_core::{
    CancellationToken, UPLOAD_PLAN_SCHEMA_VERSION, UPLOAD_PLAN_SCHEMA_VERSION_V2,
};
use immich_rs_executor::{TakeoutImportConfig, dry_run_takeout_import, dry_run_upload};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    match plan.schema_version {
        UPLOAD_PLAN_SCHEMA_VERSION => {
            let source = request
                .inputs
                .first()
                .filter(|_| request.inputs.len() == 1)
                .ok_or_else(|| CliFailure::usage("folder dry-run requires exactly one source"))?;
            let report = dry_run_upload(
                &plan,
                source,
                &request.checkpoint,
                &request.config,
                &cancellation,
            )
            .map_err(CliFailure::from_executor)?;
            output::write_json(&report, "dry-run report")
        }
        UPLOAD_PLAN_SCHEMA_VERSION_V2 => {
            let config = TakeoutImportConfig {
                source: request.takeout.clone(),
                upload: request.config.clone(),
            };
            let report = dry_run_takeout_import(
                &plan,
                &request.inputs,
                &request.checkpoint,
                &config,
                &cancellation,
            )
            .map_err(CliFailure::from_executor)?;
            output::write_json(&report, "Takeout dry-run report")
        }
        _ => Err(CliFailure::usage("unsupported upload plan schema")),
    }
}
