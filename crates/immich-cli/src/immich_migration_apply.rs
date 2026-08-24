use immich_rs_core::CancellationToken;
use immich_rs_executor::{MigrationApplyContext, apply_migration, dry_run_migration};

use crate::args::MigrationApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: MigrationApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_migration_plan(&request.plan)?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    if request.dry_run {
        let report = dry_run_migration(&plan, &request.config, &cancellation)
            .map_err(CliFailure::from_executor)?;
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
    let source_server = source
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let destination_server = destination
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    if source_server.migration_server() != plan.source_server
        || destination_server.migration_server() != plan.destination_server
    {
        return Err(CliFailure::usage(
            "migration servers do not match the immutable plan",
        ));
    }
    let destination = destination
        .authorize_import(destination_server.clone())
        .map_err(CliFailure::from_client)?;
    let report = apply_migration(
        &plan,
        checkpoint,
        &request.config,
        MigrationApplyContext::new(&source, &source_server, &destination, &destination_server),
        &cancellation,
    )
    .await
    .map_err(CliFailure::from_executor)?;
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "migration apply report")
}
