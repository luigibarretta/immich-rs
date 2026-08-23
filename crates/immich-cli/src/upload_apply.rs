use immich_rs_client::upload_plan_sha256;
use immich_rs_core::CancellationToken;
use immich_rs_core::ProductionWriteConfirmation;
use immich_rs_executor::{apply_production_upload, apply_upload};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    let production = request
        .production
        .map(|production| {
            ProductionWriteConfirmation::new(
                true,
                production.plan_sha256,
                production.expected_operations,
                production.backup_reference,
            )
            .map_err(|_| CliFailure::usage("invalid production confirmation"))
        })
        .transpose()?;
    if let Some(confirmation) = &production {
        let digest = upload_plan_sha256(&plan)
            .map_err(|_| CliFailure::usage("invalid production upload plan"))?;
        if confirmation.plan_sha256() != digest
            || confirmation.expected_operations() != plan.summary.operations
            || confirmation.expected_operations() != plan.operations.len() as u64
        {
            return Err(CliFailure::usage(
                "production confirmation does not match upload plan",
            ));
        }
    }
    let server = request
        .server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--server is required for apply"))?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let read_client = network::read_client(
        server,
        production.is_some(),
        request.ca_certificate.as_deref(),
    )?;
    let negotiated = read_client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    if negotiated.compatibility() != &plan.server {
        return Err(CliFailure::usage("server does not match upload plan"));
    }
    let report = if let Some(confirmation) = production {
        let client = read_client
            .authorize_production_upload(negotiated, &plan, &confirmation)
            .map_err(CliFailure::from_client)?;
        apply_production_upload(
            &plan,
            &request.source,
            &request.checkpoint,
            &request.config,
            &client,
            &cancellation,
        )
        .await
        .map_err(CliFailure::from_executor)?
    } else {
        let client = read_client
            .authorize_upload(negotiated)
            .map_err(CliFailure::from_client)?;
        apply_upload(
            &plan,
            &request.source,
            &request.checkpoint,
            &request.config,
            &client,
            &cancellation,
        )
        .await
        .map_err(CliFailure::from_executor)?
    };
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "apply report")
}
