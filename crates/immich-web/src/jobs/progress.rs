use std::sync::Arc;

use immich_rs_application::{ApplicationProgressEvent, ApplicationProgressObserver};

use super::{JobProgress, JobsInner, push_event};

pub(super) struct JobObserver {
    pub(super) inner: Arc<JobsInner>,
    pub(super) id: String,
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
