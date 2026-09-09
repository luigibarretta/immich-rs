use std::sync::Arc;

use immich_rs_application::{
    ApplicationErrorClass, ApplicationProgressObserver, Cancellation, FolderPlanRequest,
    FolderUploadPlanRequest, ScanError, UploadPlan, plan_folder, plan_folder_upload,
};

use super::types::JobKind;
use super::{JobStatus, JobSummary, JobsInner, apply, dry_run, progress::JobObserver, push_event};
use crate::grants::ApplyCapability;
use crate::state_store::{
    DryRunBinding, HistoryKind, PlanArtifact, SafeCounters, TerminalRecord, TerminalStatus,
    now_unix,
};

pub fn run(inner: &Arc<JobsInner>, id: &str) {
    if !begin(inner, id) {
        return;
    }
    let Some((source_id, kind, cancellation, capability)) = job_inputs(inner, id) else {
        return;
    };
    let history_kind = kind.history_kind();
    let outcome = match prepare_source(inner, &source_id) {
        Ok(request) => execute(
            inner,
            id,
            &source_id,
            kind,
            request,
            capability,
            &cancellation,
        ),
        Err(()) => WorkerOutcome::Failed(history_kind),
    };
    finish(inner, id, outcome);
}

fn job_inputs(
    inner: &JobsInner,
    id: &str,
) -> Option<(
    String,
    JobKind,
    immich_rs_application::CancellationToken,
    Option<ApplyCapability>,
)> {
    let mut state = inner.state.lock().ok()?;
    let job = state.jobs.get(id)?;
    let inputs = (
        job.source_id.clone(),
        job.kind.clone(),
        job.cancellation.clone(),
        state.apply_capabilities.remove(id),
    );
    drop(state);
    Some(inputs)
}

fn prepare_source(inner: &JobsInner, source_id: &str) -> Result<FolderPlanRequest, ()> {
    let profile = inner.config.source(source_id).ok_or(())?;
    let resolved = profile.resolve().map_err(|_| ())?;
    Ok(FolderPlanRequest {
        root: resolved.root().to_path_buf(),
        label: resolved.label().to_owned(),
        config: resolved.config().clone(),
    })
}

fn execute(
    inner: &Arc<JobsInner>,
    id: &str,
    source_id: &str,
    kind: JobKind,
    request: FolderPlanRequest,
    capability: Option<ApplyCapability>,
    cancellation: &immich_rs_application::CancellationToken,
) -> WorkerOutcome {
    if cancellation.is_cancelled() {
        return WorkerOutcome::Cancelled(kind.history_kind());
    }
    let mut observer = JobObserver {
        inner: Arc::clone(inner),
        id: id.to_owned(),
    };
    match kind {
        JobKind::Scan => match plan_folder(&request, cancellation, &mut observer) {
            Ok(plan) => WorkerOutcome::Completed {
                kind: HistoryKind::FolderScan,
                summary: normalized_summary(&plan),
                artifact: None,
                dry_run: None,
            },
            Err(ScanError::Cancelled) => WorkerOutcome::Cancelled(HistoryKind::FolderScan),
            Err(_) => WorkerOutcome::Failed(HistoryKind::FolderScan),
        },
        JobKind::Plan { server_id } => plan_upload(
            inner,
            source_id,
            &server_id,
            request,
            cancellation,
            &mut observer,
        ),
        JobKind::DryRun { reference } => {
            dry_run::execute(inner, source_id, reference, &request, cancellation)
        }
        JobKind::Apply { .. } => capability.map_or_else(
            || WorkerOutcome::Failed(HistoryKind::FolderApply),
            |value| apply::execute(inner, &request, value, cancellation),
        ),
    }
}

fn plan_upload(
    inner: &JobsInner,
    source_id: &str,
    server_id: &str,
    source: FolderPlanRequest,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> WorkerOutcome {
    let kind = HistoryKind::FolderPlan;
    let Some(source_profile) = inner.config.source(source_id) else {
        return WorkerOutcome::Failed(kind);
    };
    let Some(server_profile) = inner.config.server(server_id) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(client) = server_profile.read_client(inner.limits.dns_addresses) else {
        return WorkerOutcome::Failed(kind);
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return WorkerOutcome::Failed(kind);
    };
    let request = FolderUploadPlanRequest { source };
    let result = runtime.block_on(plan_folder_upload(
        &request,
        &client,
        cancellation,
        observer,
    ));
    let plan = match result {
        Ok(plan) => plan,
        Err(error) if error.class() == ApplicationErrorClass::Cancelled => {
            return WorkerOutcome::Cancelled(kind);
        }
        Err(_) => return WorkerOutcome::Failed(kind),
    };
    match publish_plan(inner, source_profile, server_profile, &plan) {
        Ok(artifact) => WorkerOutcome::Completed {
            kind,
            summary: upload_summary(&plan),
            artifact: Some(artifact),
            dry_run: None,
        },
        Err(()) => WorkerOutcome::Failed(kind),
    }
}

fn publish_plan(
    inner: &JobsInner,
    source: &crate::SourceProfile,
    server: &crate::ServerProfile,
    plan: &UploadPlan,
) -> Result<PlanArtifact, ()> {
    let resolved_source = source.resolve().map_err(|_| ())?;
    let state = inner
        .config
        .history_state()
        .map_err(|_| ())?
        .resolve()
        .map_err(|_| ())?;
    if !inner.store.matches_state(&state) {
        return Err(());
    }
    inner
        .store
        .plans()
        .write(
            plan,
            source.id(),
            resolved_source.generation_sha256(),
            server.id(),
            server.generation_sha256(),
            server.credential_generation(),
        )
        .map_err(|_| ())
}

const fn normalized_summary(plan: &immich_rs_application::NormalizedPlan) -> JobSummary {
    JobSummary {
        schema_version: plan.schema_version,
        assets: plan.summary.assets,
        sidecars: plan.summary.sidecars,
        bytes_read: plan.summary.bytes_read,
        warnings: plan.warnings.len(),
        errors: plan.errors.len(),
    }
}

const fn upload_summary(plan: &UploadPlan) -> JobSummary {
    JobSummary {
        schema_version: plan.schema_version,
        assets: plan.summary.operations,
        sidecars: plan.summary.xmp_sidecars,
        bytes_read: plan.summary.media_bytes,
        warnings: 0,
        errors: 0,
    }
}

fn begin(inner: &JobsInner, id: &str) -> bool {
    let Ok(mut state) = inner.state.lock() else {
        return false;
    };
    loop {
        let next_queued = state
            .order
            .iter()
            .find(|candidate| {
                state
                    .jobs
                    .get(*candidate)
                    .is_some_and(|job| job.status == JobStatus::Queued)
            })
            .map(String::as_str);
        let queued = state
            .jobs
            .get(id)
            .is_some_and(|job| job.status == JobStatus::Queued);
        if !queued || state.shutting_down {
            return false;
        }
        if next_queued == Some(id) && state.running < inner.limits.concurrent_jobs {
            state.queued = state.queued.saturating_sub(1);
            state.running = state.running.saturating_add(1);
            if let Some(job) = state.jobs.get_mut(id) {
                job.status = JobStatus::Running;
                push_event(job, inner.limits.sse_replay_events);
            }
            return true;
        }
        let Ok(next_state) = inner.worker_available.wait(state) else {
            return false;
        };
        state = next_state;
    }
}

fn finish(inner: &JobsInner, id: &str, outcome: WorkerOutcome) {
    let history_result = history_record(&outcome).and_then(|record| {
        outcome
            .dry_run_binding()
            .map_or_else(
                || inner.store.history().record_terminal(&record).map(|_| None),
                |binding| {
                    inner
                        .store
                        .history()
                        .record_dry_run(binding.clone(), &record)
                        .map(|receipt| Some(receipt.reference))
                },
            )
            .map_err(|_| ())
    });
    let history_ok = history_result.is_ok();
    if !history_ok && let Some(artifact) = outcome.published_artifact() {
        let _cleanup = inner.store.plans().remove(artifact.reference);
    }
    let receipt = history_result.ok().flatten();
    let outcome = if history_ok {
        outcome
    } else {
        WorkerOutcome::Failed(outcome.kind())
    };
    let Ok(mut state) = inner.state.lock() else {
        return;
    };
    let apply_reference = state.jobs.get(id).and_then(|job| match job.kind {
        JobKind::Apply { reference } => Some(reference),
        _ => None,
    });
    if let Some(reference) = apply_reference {
        state.active_checkpoints.remove(&reference);
    }
    state.apply_capabilities.remove(id);
    if !history_ok {
        state.worker_failed = true;
    }
    state.running = state.running.saturating_sub(1);
    if let Some(job) = state.jobs.get_mut(id) {
        match outcome {
            WorkerOutcome::Completed {
                summary, artifact, ..
            } => {
                job.status = JobStatus::Completed;
                job.summary = Some(summary);
                job.artifact = artifact;
                job.receipt = receipt;
            }
            WorkerOutcome::Failed(_) => job.status = JobStatus::Failed,
            WorkerOutcome::Cancelled(_) => job.status = JobStatus::Cancelled,
        }
        push_event(job, inner.limits.sse_replay_events);
    }
    inner.worker_available.notify_all();
}

fn history_record(outcome: &WorkerOutcome) -> Result<TerminalRecord, ()> {
    let (status, counters, plan) = match outcome {
        WorkerOutcome::Completed {
            summary, artifact, ..
        } => (
            TerminalStatus::Completed,
            SafeCounters {
                assets: summary.assets,
                sidecars: summary.sidecars,
                bytes_read: summary.bytes_read,
                warnings: u64::try_from(summary.warnings).map_err(|_| ())?,
                errors: u64::try_from(summary.errors).map_err(|_| ())?,
                max_logical_effects: artifact
                    .as_ref()
                    .map_or(0, |value| value.max_logical_effects),
            },
            artifact
                .as_ref()
                .map(|value| (value.reference, value.schema_version)),
        ),
        WorkerOutcome::Failed(_) => (TerminalStatus::Failed, SafeCounters::default(), None),
        WorkerOutcome::Cancelled(_) => (TerminalStatus::Cancelled, SafeCounters::default(), None),
    };
    Ok(TerminalRecord {
        recorded_unix: outcome.dry_run_binding().map_or_else(
            || now_unix().map_err(|_| ()),
            |binding| Ok(binding.completed_unix),
        )?,
        kind: outcome.kind(),
        status,
        plan,
        counters,
    })
}

pub(super) enum WorkerOutcome {
    Completed {
        kind: HistoryKind,
        summary: JobSummary,
        artifact: Option<PlanArtifact>,
        dry_run: Option<Box<DryRunBinding>>,
    },
    Failed(HistoryKind),
    Cancelled(HistoryKind),
}

impl WorkerOutcome {
    const fn kind(&self) -> HistoryKind {
        match self {
            Self::Completed { kind, .. } | Self::Failed(kind) | Self::Cancelled(kind) => *kind,
        }
    }

    const fn published_artifact(&self) -> Option<&PlanArtifact> {
        match self {
            Self::Completed {
                kind: HistoryKind::FolderPlan,
                artifact,
                ..
            } => artifact.as_ref(),
            Self::Failed(_) | Self::Cancelled(_) | Self::Completed { .. } => None,
        }
    }

    fn dry_run_binding(&self) -> Option<&DryRunBinding> {
        match self {
            Self::Completed { dry_run, .. } => dry_run.as_deref(),
            Self::Failed(_) | Self::Cancelled(_) => None,
        }
    }
}
