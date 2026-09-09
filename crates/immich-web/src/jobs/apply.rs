use immich_rs_application::{
    ApplicationErrorClass, UploadApplyReport, UploadDryRunReport, apply_prepared_upload,
    dry_run_upload_plan,
};

use super::worker::WorkerOutcome;
use super::{JobSummary, JobsInner, source};
use crate::grants::{ApplyCapability, binding_is_current};
use crate::state_store::{HistoryKind, PlanArtifact};

pub fn execute(
    inner: &JobsInner,
    source: &crate::ResolvedSourceProfile,
    capability: ApplyCapability,
    kind: HistoryKind,
    cancellation: &immich_rs_application::CancellationToken,
) -> WorkerOutcome {
    if !binding_is_current(&inner.config, &inner.store, &capability.binding) {
        return WorkerOutcome::Failed(kind);
    }
    let Some(server) = inner.config.server(&capability.binding.server_profile_id) else {
        return WorkerOutcome::Failed(kind);
    };
    let Some(plan_schema) = verify_source_offline(inner, source, &capability, cancellation) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(checkpoint) = inner
        .store
        .apply_checkpoint(capability.binding.dry_run.plan_reference)
    else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(client) = server.read_client(inner.limits.dns_addresses) else {
        return WorkerOutcome::Failed(kind);
    };
    let request = source::apply_request(source, checkpoint);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return WorkerOutcome::Failed(kind);
    };
    let result = runtime.block_on(apply_prepared_upload(
        capability.prepared,
        request,
        client,
        cancellation,
    ));
    let summary = match result {
        Ok(UploadApplyReport::Folder(report)) => folder_summary(report),
        Ok(UploadApplyReport::Import(report)) => import_summary(report),
        Err(error) if error.class() == ApplicationErrorClass::Cancelled => {
            return WorkerOutcome::Cancelled(kind);
        }
        _ => return WorkerOutcome::Failed(kind),
    };
    if inner
        .store
        .apply_checkpoint(capability.binding.dry_run.plan_reference)
        .is_err()
    {
        return WorkerOutcome::Failed(kind);
    }
    let Some(summary) = summary else {
        return WorkerOutcome::Failed(kind);
    };
    WorkerOutcome::Completed {
        kind,
        summary,
        artifact: Some(PlanArtifact {
            reference: capability.binding.dry_run.plan_reference,
            schema_version: plan_schema,
            plan_sha256: capability.binding.dry_run.plan_sha256,
            max_logical_effects: capability.binding.dry_run.max_logical_effects,
        }),
        dry_run: None,
    }
}

fn verify_source_offline(
    inner: &JobsInner,
    source: &crate::ResolvedSourceProfile,
    capability: &ApplyCapability,
    cancellation: &immich_rs_application::CancellationToken,
) -> Option<u32> {
    let Ok(stored) = inner
        .store
        .plans()
        .load(capability.binding.dry_run.plan_reference)
    else {
        return None;
    };
    let Ok(checkpoint) = inner.store.unused_checkpoint() else {
        return None;
    };
    let request = source::dry_run_request(source, checkpoint);
    let valid = match dry_run_upload_plan(&stored.plan, &request, cancellation) {
        Ok(UploadDryRunReport::Folder(report)) => valid_folder_dry_run(&report),
        Ok(UploadDryRunReport::Import(report)) => valid_import_dry_run(&report, &stored.plan),
        Err(_) => false,
    };
    let checkpoint_unused = inner
        .store
        .verify_checkpoint_unused(&request.checkpoint)
        .is_ok();
    (valid && checkpoint_unused).then_some(stored.plan.schema_version)
}

const fn valid_folder_dry_run(report: &immich_rs_application::ApplyReport) -> bool {
    report.dry_run
        && !report.cancelled
        && report.created == 0
        && report.duplicate == 0
        && report.retried == 0
        && report.failed == 0
        && report.indeterminate == 0
        && report.planned == report.would_upload.saturating_add(report.resumed)
}

fn valid_import_dry_run(
    report: &immich_rs_application::ImportApplyReport,
    plan: &immich_rs_application::UploadPlan,
) -> bool {
    report.dry_run
        && !report.cancelled
        && report.created == 0
        && report.duplicate == 0
        && report.metadata_updated == 0
        && report.albums_created == 0
        && report.albums_reused == 0
        && report.album_memberships_updated == 0
        && report.resumed_effects == 0
        && report.retried == 0
        && report.failed == 0
        && report.indeterminate == 0
        && report.planned == plan.summary
        && report.would_upload == report.planned.operations
        && report.would_update_metadata == report.planned.metadata_updates
        && report.would_create_albums == report.planned.album_creates
        && report.would_add_album_memberships == report.planned.album_memberships
}

fn folder_summary(report: immich_rs_application::ApplyReport) -> Option<JobSummary> {
    Some(JobSummary {
        schema_version: report.schema_version,
        assets: report.created.saturating_add(report.duplicate),
        sidecars: 0,
        bytes_read: 0,
        warnings: usize::try_from(report.retried).ok()?,
        errors: usize::try_from(report.failed.saturating_add(report.indeterminate)).ok()?,
    })
}

fn import_summary(report: immich_rs_application::ImportApplyReport) -> Option<JobSummary> {
    Some(JobSummary {
        schema_version: report.schema_version,
        assets: report.created.saturating_add(report.duplicate),
        sidecars: 0,
        bytes_read: 0,
        warnings: usize::try_from(report.retried).ok()?,
        errors: usize::try_from(report.failed.saturating_add(report.indeterminate)).ok()?,
    })
}
