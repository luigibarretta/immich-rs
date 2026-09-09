use immich_rs_application::{CancellationToken, plan_migration};

use crate::args::MigrationPlanRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: MigrationPlanRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let (source, destination) =
        network::migration_read_clients(&request.source_server, &request.destination_server)?;
    let plan = plan_migration(&source, &destination, request.config, &cancellation)
        .await
        .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&plan, "migration plan")
}
