use immich_rs_core::CancellationToken;
use immich_rs_executor::apply_upload;

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let server = request
        .server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--server is required for apply"))?;
    let read_client = network::read_client(server, false)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let negotiated = read_client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    if negotiated.compatibility() != &plan.server {
        return Err(CliFailure::usage("server does not match upload plan"));
    }
    let client = read_client
        .authorize_upload(negotiated)
        .map_err(CliFailure::from_client)?;
    let report = apply_upload(
        &plan,
        &request.source,
        &request.checkpoint,
        &request.config,
        &client,
        &cancellation,
    )
    .await
    .map_err(CliFailure::from_executor)?;
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "apply report")
}
