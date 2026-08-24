use immich_rs_core::CancellationToken;
use immich_rs_executor::create_migration_plan;

use crate::args::MigrationPlanRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: MigrationPlanRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let (source, destination) =
        network::migration_read_clients(&request.source_server, &request.destination_server)?;
    let source_server = source
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let destination_server = destination
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let plan = create_migration_plan(
        &source,
        &source_server,
        &destination_server,
        request.config,
        &cancellation,
    )
    .await
    .map_err(CliFailure::from_executor)?;
    output::write_json(&plan, "migration plan")
}
