mod subscription;
mod worker;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use immich_rs_application::{CancellationToken, ProgressStage};
use tokio::sync::broadcast;

use crate::{WebConfig, WebLimits};

pub use subscription::{JobSubscription, SubscribeError, SubscriptionDelivery};

const JOB_ID_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JobProgress {
    pub sequence: u64,
    pub stage: Option<ProgressStage>,
    pub assets_observed: u64,
    pub bytes_read: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobSummary {
    pub schema_version: u32,
    pub assets: u64,
    pub sidecars: u64,
    pub bytes_read: u64,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSnapshot {
    pub id: String,
    pub source_label: String,
    pub status: JobStatus,
    pub cancellation_requested: bool,
    pub progress: JobProgress,
    pub summary: Option<JobSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobEvent {
    pub sequence: u64,
    pub status: JobStatus,
    pub cancellation_requested: bool,
    pub progress: JobProgress,
    pub summary: Option<JobSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    Full,
    UnknownProfile,
    Unavailable,
}

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<JobsInner>,
}

pub struct JobsInner {
    pub config: Arc<WebConfig>,
    pub limits: WebLimits,
    pub state: Mutex<JobsState>,
    pub worker_available: Condvar,
}

pub struct JobsState {
    pub jobs: BTreeMap<String, StoredJob>,
    order: VecDeque<String>,
    handles: Vec<OwnedWorker>,
    pub running: usize,
    pub queued: usize,
    pub shutting_down: bool,
    worker_failed: bool,
    subscribers: usize,
    subscribers_by_owner: BTreeMap<[u8; 32], usize>,
}

pub struct StoredJob {
    pub id: String,
    pub owner: [u8; 32],
    pub source_id: String,
    pub source_label: String,
    pub status: JobStatus,
    pub cancellation_requested: bool,
    pub cancellation: CancellationToken,
    pub progress: JobProgress,
    pub summary: Option<JobSummary>,
    pub events: VecDeque<JobEvent>,
    pub event_sender: broadcast::Sender<JobEvent>,
    next_event_sequence: u64,
}

struct OwnedWorker {
    handle: JoinHandle<()>,
}

impl JobManager {
    pub fn new(config: Arc<WebConfig>) -> Self {
        let limits = config.limits();
        Self {
            inner: Arc::new(JobsInner {
                config,
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
        let cancellation = CancellationToken::default();
        let mut stored = StoredJob {
            id: id.clone(),
            owner,
            source_id: source_id.to_owned(),
            source_label,
            status: JobStatus::Queued,
            cancellation_requested: false,
            cancellation,
            progress: JobProgress::default(),
            summary: None,
            events: VecDeque::with_capacity(self.inner.limits.sse_replay_events),
            event_sender,
            next_event_sequence: 0,
        };
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
        {
            let job = state.jobs.get_mut(id).filter(|job| &job.owner == owner)?;
            if !job.status.terminal() && !job.cancellation_requested {
                job.cancellation_requested = true;
                job.cancellation.cancel();
                if was_queued {
                    job.status = JobStatus::Cancelled;
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

pub fn push_event(job: &mut StoredJob, replay_limit: usize) {
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

fn random_job_id() -> Result<String, ()> {
    let mut bytes = [0_u8; JOB_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    let id = URL_SAFE_NO_PAD.encode(bytes);
    bytes.fill(0);
    Ok(id)
}
