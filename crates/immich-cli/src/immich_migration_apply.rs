use immich_rs_application::{CancellationToken, apply_migration_plan, dry_run_migration_plan};

use crate::args::MigrationApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: MigrationApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_migration_plan(&request.plan)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    if request.dry_run {
        let report = dry_run_migration_plan(&plan, &request.config, &cancellation)
            .map_err(|error| CliFailure::from_application(&error))?;
        return output::write_json(&report, "migration dry-run report");
    }
    let checkpoint = request
        .checkpoint
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--checkpoint is required for apply"))?;
    let source_origin = request
        .source_server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--source-server is required for apply"))?;
    let destination_origin = request
        .destination_server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--destination-server is required for apply"))?;
    let (source, destination) = network::migration_read_clients(source_origin, destination_origin)?;
    let report = apply_migration_plan(
        &plan,
        checkpoint,
        &request.config,
        &source,
        destination,
        &cancellation,
    )
    .await
    .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&report, "migration apply report")
}
