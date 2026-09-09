use immich_rs_application::{
    CancellationToken, ProductionWriteRequest, UploadApplyReport, UploadApplyRequest,
    apply_prepared_upload, prepare_upload_apply,
};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let production = request.production.map(|production| ProductionWriteRequest {
        plan_sha256: production.plan_sha256,
        expected_operations: production.expected_operations,
        backup_reference: production.backup_reference,
    });
    let prepared = prepare_upload_apply(plan, production)
        .map_err(|error| CliFailure::from_application(&error))?;
    let server = request
        .server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--server is required for apply"))?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let read_client = network::read_client(
        server,
        prepared.is_production(),
        request.ca_certificate.as_deref(),
    )?;
    let application_request = UploadApplyRequest {
        inputs: request.inputs,
        checkpoint: request.checkpoint,
        config: request.config,
        takeout: request.takeout,
        apple: request.apple,
        picasa: request.picasa,
    };
    match apply_prepared_upload(prepared, application_request, read_client, &cancellation)
        .await
        .map_err(|error| CliFailure::from_application(&error))?
    {
        UploadApplyReport::Folder(report) => output::write_json(&report, "apply report"),
        UploadApplyReport::Import(report) => output::write_json(&report, "import apply report"),
    }
}
