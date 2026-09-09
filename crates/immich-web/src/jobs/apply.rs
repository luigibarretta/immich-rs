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
    if source.settings().kind() != crate::SourceKind::Folder {
        return WorkerOutcome::Failed(kind);
    }
    if !binding_is_current(&inner.config, &inner.store, &capability.binding) {
        return WorkerOutcome::Failed(kind);
    }
    let Some(server) = inner.config.server(&capability.binding.server_profile_id) else {
        return WorkerOutcome::Failed(kind);
    };
    if !verify_source_offline(inner, source, &capability, cancellation) {
        return WorkerOutcome::Failed(kind);
    }
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
    let report = match result {
        Ok(UploadApplyReport::Folder(report)) => report,
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
    let Ok(errors) = usize::try_from(report.failed.saturating_add(report.indeterminate)) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(warnings) = usize::try_from(report.retried) else {
        return WorkerOutcome::Failed(kind);
    };
    WorkerOutcome::Completed {
        kind,
        summary: JobSummary {
            schema_version: report.schema_version,
            assets: report.created.saturating_add(report.duplicate),
            sidecars: 0,
            bytes_read: 0,
            warnings,
            errors,
        },
        artifact: Some(PlanArtifact {
            reference: capability.binding.dry_run.plan_reference,
            schema_version: 1,
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
) -> bool {
    let Ok(stored) = inner
        .store
        .plans()
        .load(capability.binding.dry_run.plan_reference)
    else {
        return false;
    };
    let Ok(checkpoint) = inner
        .store
        .apply_checkpoint_candidate(capability.binding.dry_run.plan_reference)
    else {
        return false;
    };
    let request = source::dry_run_request(source, checkpoint);
    matches!(
        dry_run_upload_plan(&stored.plan, &request, cancellation),
        Ok(UploadDryRunReport::Folder(_))
    )
}
