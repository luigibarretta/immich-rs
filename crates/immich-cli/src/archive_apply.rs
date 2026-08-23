use immich_rs_core::CancellationToken;
use immich_rs_executor::apply_archive;

use crate::args::ArchiveApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ArchiveApplyRequest) -> Result<(), CliFailure> {
    let manifest = output::load_archive_manifest(&request.manifest)?;
    let client = network::archive_client(&request.server, request.production_read)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let negotiated = client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let report = apply_archive(
        &manifest,
        &request.destination,
        &client,
        &negotiated,
        &cancellation,
    )
    .await
    .map_err(CliFailure::from_executor)?;
    output::write_json(&report, "archive apply report")
}
