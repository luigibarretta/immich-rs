mod subscription;
mod types;
mod worker;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use tokio::sync::broadcast;

use crate::state_store::{
    ConsoleStore, HistoryKind, SafeCounters, TerminalRecord, TerminalStatus, now_unix,
};
use crate::{WebConfig, WebLimits};

pub use subscription::{JobSubscription, SubscribeError, SubscriptionDelivery};
pub use types::{AdmissionError, JobEvent, JobProgress, JobSnapshot, JobStatus, JobSummary};
use types::{JobKind, StoredJob, random_job_id};

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<JobsInner>,
}

struct JobsInner {
    config: Arc<WebConfig>,
    store: Arc<ConsoleStore>,
    limits: WebLimits,
    state: Mutex<JobsState>,
    worker_available: Condvar,
}

struct JobsState {
    jobs: BTreeMap<String, StoredJob>,
    order: VecDeque<String>,
    handles: Vec<OwnedWorker>,
    running: usize,
    queued: usize,
    shutting_down: bool,
    worker_failed: bool,
    subscribers: usize,
    subscribers_by_owner: BTreeMap<[u8; 32], usize>,
}

struct OwnedWorker {
    handle: JoinHandle<()>,
}

impl JobManager {
    pub fn new(config: Arc<WebConfig>, store: Arc<ConsoleStore>) -> Self {
        let limits = config.limits();
        Self {
            inner: Arc::new(JobsInner {
                config,
                store,
                limits,
                state: Mutex::new(JobsState {
                    jobs: BTreeMap::new(),
                    order: VecDeque::new(),
                    handles: Vec::new(),
                    running: 0,
                    queued: 0,
                    shutting_down: false,
                    worker_failed: false,
                    subscribers: 0,
                    subscribers_by_owner: BTreeMap::new(),
                }),
                worker_available: Condvar::new(),
            }),
        }
    }

    pub fn admit(&self, owner: [u8; 32], source_id: &str) -> Result<String, AdmissionError> {
        self.admit_kind(owner, source_id, JobKind::FolderScan)
    }

    pub fn admit_plan(
        &self,
        owner: [u8; 32],
        source_id: &str,
        server_id: &str,
    ) -> Result<String, AdmissionError> {
        if self.inner.config.server(server_id).is_none() {
            return Err(AdmissionError::UnknownProfile);
        }
        self.admit_kind(
            owner,
            source_id,
            JobKind::FolderPlan {
                server_id: server_id.to_owned(),
            },
        )
    }

    fn admit_kind(
        &self,
        owner: [u8; 32],
        source_id: &str,
        kind: JobKind,
    ) -> Result<String, AdmissionError> {
        let source_label = self
            .inner
            .config
            .source(source_id)
            .map(|profile| profile.label().to_owned())
            .ok_or(AdmissionError::UnknownProfile)?;
        let id = random_job_id().map_err(|()| AdmissionError::Unavailable)?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| AdmissionError::Unavailable)?;
        reap_workers(&mut state);
        evict_terminal(&mut state, self.inner.limits.retained_jobs);
        let worker_limit = self
            .inner
            .limits
            .concurrent_jobs
            .saturating_add(self.inner.limits.queued_jobs);
        if state.shutting_down || state.worker_failed {
            return Err(AdmissionError::Unavailable);
        }
        if state.jobs.contains_key(&id)
            || state.running.saturating_add(state.queued) >= worker_limit
            || state.handles.len() >= worker_limit
            || state.jobs.len() >= self.inner.limits.retained_jobs
        {
            return Err(AdmissionError::Full);
        }
        let (event_sender, _receiver) = broadcast::channel(self.inner.limits.sse_replay_events);
        let mut stored = StoredJob::new(
            id.clone(),
            owner,
            source_id.to_owned(),
            source_label,
            kind,
            event_sender,
            self.inner.limits.sse_replay_events,
        );
        push_event(&mut stored, self.inner.limits.sse_replay_events);
        state.jobs.insert(id.clone(), stored);
        state.order.push_back(id.clone());
        state.queued = state.queued.saturating_add(1);
        let worker_inner = Arc::clone(&self.inner);
        let worker_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("immich-web-job".to_owned())
            .spawn(move || worker::run(&worker_inner, &worker_id));
        let Ok(handle) = spawned else {
            state.jobs.remove(&id);
            state.order.retain(|candidate| candidate != &id);
            state.queued = state.queued.saturating_sub(1);
            return Err(AdmissionError::Unavailable);
        };
        state.handles.push(OwnedWorker { handle });
        drop(state);
        Ok(id)
    }

    pub fn snapshot(&self, id: &str, owner: &[u8; 32]) -> Option<JobSnapshot> {
        let mut state = self.inner.state.lock().ok()?;
        reap_workers(&mut state);
        let snapshot = state
            .jobs
            .get(id)
            .filter(|job| &job.owner == owner)
            .map(snapshot_job)?;
        drop(state);
        Some(snapshot)
    }

    pub fn cancel(&self, id: &str, owner: &[u8; 32]) -> Option<JobSnapshot> {
        let mut state = self.inner.state.lock().ok()?;
        let was_queued = state
            .jobs
            .get(id)
            .filter(|job| &job.owner == owner)
            .is_some_and(|job| job.status == JobStatus::Queued);
        let mut record_cancelled = None;
        {
            let job = state.jobs.get_mut(id).filter(|job| &job.owner == owner)?;
            if !job.status.terminal() && !job.cancellation_requested {
                job.cancellation_requested = true;
                job.cancellation.cancel();
                if was_queued {
                    job.status = JobStatus::Cancelled;
                    record_cancelled = Some(job.kind.history_kind());
                }
                push_event(job, self.inner.limits.sse_replay_events);
                self.inner.worker_available.notify_all();
            }
        }
        if was_queued {
            state.queued = state.queued.saturating_sub(1);
        }
        let job = state.jobs.get(id).filter(|job| &job.owner == owner)?;
        let snapshot = snapshot_job(job);
        drop(state);
        if record_cancelled.is_some_and(|kind| record_cancelled_job(&self.inner, kind).is_err()) {
            if let Ok(mut state) = self.inner.state.lock() {
                state.worker_failed = true;
            }
        }
        Some(snapshot)
    }

    pub fn shutdown(&self) -> bool {
        let (handles, mut clean) = {
            let Ok(mut state) = self.inner.state.lock() else {
                return false;
            };
            state.shutting_down = true;
            let mut cancelled_queued = 0_usize;
            for job in state
                .jobs
                .values_mut()
                .filter(|job| !job.status.terminal() && !job.cancellation_requested)
            {
                job.cancellation_requested = true;
                job.cancellation.cancel();
                if job.status == JobStatus::Queued {
                    job.status = JobStatus::Cancelled;
                    cancelled_queued = cancelled_queued.saturating_add(1);
                }
                push_event(job, self.inner.limits.sse_replay_events);
            }
            state.queued = state.queued.saturating_sub(cancelled_queued);
            self.inner.worker_available.notify_all();
            let clean = !state.worker_failed;
            (std::mem::take(&mut state.handles), clean)
        };
        for worker in handles {
            clean &= worker.handle.join().is_ok();
        }
        clean
    }

    #[cfg(test)]
    pub fn owned_worker_count(&self) -> Option<usize> {
        self.inner
            .state
            .lock()
            .ok()
            .map(|state| state.handles.len())
    }
}

fn record_cancelled_job(inner: &JobsInner, kind: HistoryKind) -> Result<(), crate::WebConfigError> {
    inner.store.history().record_terminal(&TerminalRecord {
        recorded_unix: now_unix()?,
        kind,
        status: TerminalStatus::Cancelled,
        plan: None,
        counters: SafeCounters::default(),
    })?;
    Ok(())
}

fn push_event(job: &mut StoredJob, replay_limit: usize) {
    job.next_event_sequence = job.next_event_sequence.saturating_add(1);
    let event = JobEvent {
        sequence: job.next_event_sequence,
        status: job.status,
        cancellation_requested: job.cancellation_requested,
        progress: job.progress,
        summary: job.summary,
    };
    if job.events.len() == replay_limit {
        job.events.pop_front();
    }
    job.events.push_back(event);
    let _subscriber_count = job.event_sender.send(event);
}

fn snapshot_job(job: &StoredJob) -> JobSnapshot {
    JobSnapshot {
        id: job.id.clone(),
        source_label: job.source_label.clone(),
        status: job.status,
        cancellation_requested: job.cancellation_requested,
        progress: job.progress,
        summary: job.summary,
        artifact: job.artifact.clone(),
    }
}

fn reap_workers(state: &mut JobsState) {
    let mut position = 0;
    while position < state.handles.len() {
        if state.handles[position].handle.is_finished() {
            let worker = state.handles.swap_remove(position);
            if worker.handle.join().is_err() {
                state.worker_failed = true;
            }
        } else {
            position += 1;
        }
    }
}

fn evict_terminal(state: &mut JobsState, retained: usize) {
    while state.jobs.len() >= retained {
        let Some(position) = state
            .order
            .iter()
            .position(|id| state.jobs.get(id).is_some_and(|job| job.status.terminal()))
        else {
            break;
        };
        if let Some(id) = state.order.remove(position) {
            state.jobs.remove(&id);
        }
    }
}
