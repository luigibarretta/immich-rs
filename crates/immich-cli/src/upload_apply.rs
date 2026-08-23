use immich_rs_client::{ImmichReadClient, NegotiatedServer, upload_plan_sha256};
use immich_rs_core::{
    CancellationToken, ProductionWriteConfirmation, UPLOAD_PLAN_SCHEMA_VERSION,
    UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan,
};
use immich_rs_executor::{
    TakeoutImportConfig, apply_production_upload, apply_takeout_import, apply_upload,
};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    if !matches!(
        plan.schema_version,
        UPLOAD_PLAN_SCHEMA_VERSION | UPLOAD_PLAN_SCHEMA_VERSION_V2
    ) {
        return Err(CliFailure::usage(
            "apply upload does not support this plan schema",
        ));
    }
    if plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2 && request.production.is_some() {
        return Err(CliFailure::usage(
            "production Takeout import is not yet authorized",
        ));
    }
    let production = request
        .production
        .as_ref()
        .map(|production| {
            ProductionWriteConfirmation::new(
                true,
                production.plan_sha256.clone(),
                production.expected_operations,
                production.backup_reference.clone(),
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
    if plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2 {
        return run_takeout(request, &plan, read_client, negotiated, &cancellation).await;
    }
    let source = request
        .inputs
        .first()
        .filter(|_| request.inputs.len() == 1)
        .ok_or_else(|| CliFailure::usage("folder apply requires exactly one source"))?;
    let report = if let Some(confirmation) = production {
        let client = read_client
            .authorize_production_upload(negotiated, &plan, &confirmation)
            .map_err(CliFailure::from_client)?;
        apply_production_upload(
            &plan,
            source,
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
            source,
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

async fn run_takeout(
    request: ApplyRequest,
    plan: &UploadPlan,
    read_client: ImmichReadClient,
    negotiated: NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<(), CliFailure> {
    let config = TakeoutImportConfig {
        source: request.takeout,
        upload: request.config,
    };
    let client = read_client
        .authorize_import(negotiated)
        .map_err(CliFailure::from_client)?;
    let report = apply_takeout_import(
        plan,
        &request.inputs,
        &request.checkpoint,
        &config,
        &client,
        cancellation,
    )
    .await
    .map_err(CliFailure::from_executor)?;
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "Takeout apply report")
}
