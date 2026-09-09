use super::{JobManager, JobStatus, reap_workers};

#[derive(Clone, Copy)]
pub struct JobMetrics {
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub retained: usize,
    pub subscribers: usize,
}

pub(super) fn collect(manager: &JobManager) -> Option<JobMetrics> {
    let mut state = manager.inner.state.lock().ok()?;
    reap_workers(&mut state);
    let mut completed = 0_usize;
    let mut failed = 0_usize;
    let mut cancelled = 0_usize;
    for job in state.jobs.values() {
        match job.status {
            JobStatus::Completed => completed = completed.saturating_add(1),
            JobStatus::Failed => failed = failed.saturating_add(1),
            JobStatus::Cancelled => cancelled = cancelled.saturating_add(1),
            JobStatus::Queued | JobStatus::Running => {}
        }
    }
    Some(JobMetrics {
        queued: state.queued,
        running: state.running,
        completed,
        failed,
        cancelled,
        retained: state.jobs.len(),
        subscribers: state.subscribers,
    })
}
