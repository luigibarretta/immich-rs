use std::sync::Arc;

use immich_rs_application::{
    ApplicationProgressEvent, ApplicationProgressObserver, Cancellation, FolderPlanRequest,
    ScanError, plan_folder,
};

use super::{JobProgress, JobStatus, JobSummary, JobsInner, push_event};
use crate::state_store::{HistoryKind, SafeCounters, TerminalRecord, TerminalStatus, now_unix};

pub fn run(inner: &Arc<JobsInner>, id: &str) {
    if !begin(inner, id) {
        return;
    }
    let request = {
        let Ok(state) = inner.state.lock() else {
            return;
        };
        let Some(job) = state.jobs.get(id) else {
            return;
        };
        let Some(profile) = inner.config.source(&job.source_id) else {
            drop(state);
            finish(inner, id, WorkerOutcome::Failed);
            return;
        };
        let Ok(resolved) = profile.resolve() else {
            drop(state);
            finish(inner, id, WorkerOutcome::Failed);
            return;
        };
        FolderPlanRequest {
            root: resolved.root().to_path_buf(),
            label: resolved.label().to_owned(),
            config: resolved.config().clone(),
        }
    };
    let cancellation = {
        let Ok(state) = inner.state.lock() else {
            return;
        };
        let Some(job) = state.jobs.get(id) else {
            return;
        };
        job.cancellation.clone()
    };
    if cancellation.is_cancelled() {
        finish(inner, id, WorkerOutcome::Cancelled);
        return;
    }
    let mut observer = JobObserver {
        inner: Arc::clone(inner),
        id: id.to_owned(),
    };
    let outcome = match plan_folder(&request, &cancellation, &mut observer) {
        Ok(plan) => WorkerOutcome::Completed(JobSummary {
            schema_version: plan.schema_version,
            assets: plan.summary.assets,
            sidecars: plan.summary.sidecars,
            bytes_read: plan.summary.bytes_read,
            warnings: plan.warnings.len(),
            errors: plan.errors.len(),
        }),
        Err(ScanError::Cancelled) => WorkerOutcome::Cancelled,
        Err(_) => WorkerOutcome::Failed,
    };
    finish(inner, id, outcome);
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
    let history_ok = history_record(outcome).and_then(|record| {
        inner
            .store
            .history()
            .record_terminal(&record)
            .map(|_| ())
            .map_err(|_| ())
    });
    let outcome = if history_ok.is_ok() {
        outcome
    } else {
        WorkerOutcome::Failed
    };
    let Ok(mut state) = inner.state.lock() else {
        return;
    };
    if history_ok.is_err() {
        state.worker_failed = true;
    }
    state.running = state.running.saturating_sub(1);
    if let Some(job) = state.jobs.get_mut(id) {
        match outcome {
            WorkerOutcome::Completed(summary) => {
                job.status = JobStatus::Completed;
                job.summary = Some(summary);
            }
            WorkerOutcome::Failed => job.status = JobStatus::Failed,
            WorkerOutcome::Cancelled => job.status = JobStatus::Cancelled,
        }
        push_event(job, inner.limits.sse_replay_events);
    }
    inner.worker_available.notify_all();
}

fn history_record(outcome: WorkerOutcome) -> Result<TerminalRecord, ()> {
    let (status, counters) = match outcome {
        WorkerOutcome::Completed(summary) => (
            TerminalStatus::Completed,
            SafeCounters {
                assets: summary.assets,
                sidecars: summary.sidecars,
                bytes_read: summary.bytes_read,
                warnings: u64::try_from(summary.warnings).map_err(|_| ())?,
                errors: u64::try_from(summary.errors).map_err(|_| ())?,
                max_logical_effects: 0,
            },
        ),
        WorkerOutcome::Failed => (TerminalStatus::Failed, SafeCounters::default()),
        WorkerOutcome::Cancelled => (TerminalStatus::Cancelled, SafeCounters::default()),
    };
    Ok(TerminalRecord {
        recorded_unix: now_unix().map_err(|_| ())?,
        kind: HistoryKind::FolderScan,
        status,
        plan: None,
        counters,
    })
}

struct JobObserver {
    inner: Arc<JobsInner>,
    id: String,
}

impl ApplicationProgressObserver for JobObserver {
    fn observe(&mut self, event: ApplicationProgressEvent) {
        let ApplicationProgressEvent::Scan { event, .. } = event else {
            return;
        };
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if let Some(job) = state.jobs.get_mut(&self.id) {
            job.progress = JobProgress {
                sequence: event.sequence,
                stage: Some(event.stage),
                assets_observed: event.assets_observed,
                bytes_read: event.bytes_read,
            };
            push_event(job, self.inner.limits.sse_replay_events);
        }
    }
}

#[derive(Clone, Copy)]
enum WorkerOutcome {
    Completed(JobSummary),
    Failed,
    Cancelled,
}
