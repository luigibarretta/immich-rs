use immich_rs_application::{
    CancellationToken, UploadDryRunReport, UploadDryRunRequest, dry_run_upload_plan,
};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let application_request = UploadDryRunRequest {
        inputs: request.inputs.clone(),
        checkpoint: request.checkpoint.clone(),
        config: request.config.clone(),
        takeout: request.takeout.clone(),
        apple: request.apple.clone(),
        picasa: request.picasa.clone(),
    };
    match dry_run_upload_plan(&plan, &application_request, &cancellation)
        .map_err(|error| CliFailure::from_application(&error))?
    {
        UploadDryRunReport::Folder(report) => output::write_json(&report, "dry-run report"),
        UploadDryRunReport::Import(report) => output::write_json(&report, "import dry-run report"),
    }
}
