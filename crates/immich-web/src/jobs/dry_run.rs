use immich_rs_application::{
    ApplePhotosScanConfig, ApplicationErrorClass, Cancellation, FolderPlanRequest,
    PicasaScanConfig, TakeoutScanConfig, UploadDryRunReport, UploadDryRunRequest,
    UploadExecutionConfig, dry_run_upload_plan,
};
use sha2::{Digest, Sha256};

use super::worker::WorkerOutcome;
use super::{JobSummary, JobsInner};
use crate::state_store::{ArtifactRef, DryRunBinding, HistoryKind, now_unix};

pub(super) fn execute(
    inner: &JobsInner,
    source_id: &str,
    reference: ArtifactRef,
    source: &FolderPlanRequest,
    cancellation: &impl Cancellation,
) -> WorkerOutcome {
    let kind = HistoryKind::FolderDryRun;
    let Some(source_profile) = inner.config.source(source_id) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(stored) = inner.store.plans().load(reference) else {
        return WorkerOutcome::Failed(kind);
    };
    let Some(server_profile) = inner.config.server(&stored.binding.server_profile_id) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(source_generation) = current_source_generation(inner, source_profile) else {
        return WorkerOutcome::Failed(kind);
    };
    if stored.binding.source_profile_id != source_id
        || source_generation != stored.binding.source_profile_sha256
        || server_profile.generation_sha256() != stored.binding.server_profile_sha256
        || server_profile.credential_generation() != stored.binding.credential_generation
    {
        return WorkerOutcome::Failed(kind);
    }
    let (report_schema, planned) = match verify_offline(inner, &stored.plan, source, cancellation) {
        OfflineOutcome::Completed { schema, planned } => (schema, planned),
        OfflineOutcome::Cancelled => return WorkerOutcome::Cancelled(kind),
        OfflineOutcome::Failed => return WorkerOutcome::Failed(kind),
    };
    let Ok(current_generation) = current_source_generation(inner, source_profile) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(state) = inner
        .config
        .history_state()
        .and_then(crate::StateProfile::resolve)
    else {
        return WorkerOutcome::Failed(kind);
    };
    if current_generation != source_generation
        || !inner.store.matches_state(&state)
        || server_profile.generation_sha256() != stored.binding.server_profile_sha256
        || server_profile.credential_generation() != stored.binding.credential_generation
    {
        return WorkerOutcome::Failed(kind);
    }
    let Ok(completed_unix) = now_unix() else {
        return WorkerOutcome::Failed(kind);
    };
    let binding = DryRunBinding {
        plan_reference: reference,
        plan_sha256: stored.binding.plan_sha256.clone(),
        source_configuration_sha256: source_configuration_digest(
            &source_generation,
            &stored.plan.configuration_sha256,
        ),
        server_identity_sha256: stored.binding.server_identity_sha256.clone(),
        server_profile_sha256: stored.binding.server_profile_sha256.clone(),
        credential_generation: stored.binding.credential_generation,
        max_logical_effects: stored.binding.max_logical_effects,
        completed_unix,
    };
    WorkerOutcome::Completed {
        kind,
        summary: JobSummary {
            schema_version: report_schema,
            assets: planned,
            sidecars: stored.plan.summary.xmp_sidecars,
            bytes_read: stored.plan.summary.media_bytes,
            warnings: 0,
            errors: 0,
        },
        artifact: Some(crate::state_store::PlanArtifact {
            reference,
            schema_version: stored.plan.schema_version,
            plan_sha256: stored.binding.plan_sha256,
            max_logical_effects: stored.binding.max_logical_effects,
        }),
        dry_run: Some(Box::new(binding)),
    }
}

enum OfflineOutcome {
    Completed { schema: u32, planned: u64 },
    Cancelled,
    Failed,
}

fn verify_offline(
    inner: &JobsInner,
    plan: &immich_rs_application::UploadPlan,
    source: &FolderPlanRequest,
    cancellation: &impl Cancellation,
) -> OfflineOutcome {
    let Ok(checkpoint) = inner.store.unused_checkpoint() else {
        return OfflineOutcome::Failed;
    };
    let mut config = UploadExecutionConfig::default();
    config.scan = source.config.clone();
    let request = UploadDryRunRequest {
        inputs: vec![source.root.clone()],
        checkpoint: checkpoint.clone(),
        config,
        takeout: TakeoutScanConfig::default(),
        apple: ApplePhotosScanConfig::default(),
        picasa: PicasaScanConfig::default(),
    };
    let report = match dry_run_upload_plan(plan, &request, cancellation) {
        Ok(UploadDryRunReport::Folder(report)) => report,
        Err(error) if error.class() == ApplicationErrorClass::Cancelled => {
            return OfflineOutcome::Cancelled;
        }
        _ => return OfflineOutcome::Failed,
    };
    let valid = report.dry_run
        && !report.cancelled
        && report.created == 0
        && report.duplicate == 0
        && report.retried == 0
        && report.failed == 0
        && report.indeterminate == 0
        && report.planned == report.would_upload.saturating_add(report.resumed)
        && inner.store.verify_checkpoint_unused(&checkpoint).is_ok();
    if valid {
        OfflineOutcome::Completed {
            schema: report.schema_version,
            planned: report.planned,
        }
    } else {
        OfflineOutcome::Failed
    }
}

fn current_source_generation(
    inner: &JobsInner,
    profile: &crate::SourceProfile,
) -> Result<String, ()> {
    let resolved = profile.resolve().map_err(|_| ())?;
    let state = inner
        .config
        .history_state()
        .map_err(|_| ())?
        .resolve()
        .map_err(|_| ())?;
    if !inner.store.matches_state(&state) {
        return Err(());
    }
    Ok(resolved.generation_sha256().to_owned())
}

fn source_configuration_digest(source: &str, configuration: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"immich-rs-web-source-configuration-v1\0");
    digest.update(source.as_bytes());
    digest.update([0]);
    digest.update(configuration.as_bytes());
    format!("{:x}", digest.finalize())
}
