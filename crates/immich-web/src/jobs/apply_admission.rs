use std::sync::Arc;

use tokio::sync::broadcast;

use super::{
    AdmissionError, JobKind, JobManager, StoredJob, evict_terminal, push_event, random_job_id,
    reap_workers,
};
use crate::grants::ApplyCapability;

impl JobManager {
    pub fn admit_apply(&self, capability: ApplyCapability) -> Result<String, AdmissionError> {
        let owner = capability.binding.owner;
        let source_id = capability.binding.source_profile_id.clone();
        let reference = capability.binding.dry_run.plan_reference;
        let Some(source_label) = self
            .inner
            .config
            .source(&source_id)
            .map(|profile| profile.label().to_owned())
        else {
            return Err(AdmissionError::UnknownProfile);
        };
        let history = self
            .inner
            .config
            .source(&source_id)
            .map(|profile| super::source::apply_history(profile.kind()))
            .ok_or(AdmissionError::UnknownProfile)?;
        let Ok(id) = random_job_id() else {
            return Err(AdmissionError::Unavailable);
        };
        let Ok(mut state) = self.inner.state.lock() else {
            return Err(AdmissionError::Unavailable);
        };
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
        if state.active_checkpoints.contains(&reference)
            || state.jobs.contains_key(&id)
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
            source_id,
            source_label,
            JobKind::Apply { reference, history },
            event_sender,
            self.inner.limits.sse_replay_events,
        );
        push_event(&mut stored, self.inner.limits.sse_replay_events);
        state.jobs.insert(id.clone(), stored);
        state.order.push_back(id.clone());
        state.queued = state.queued.saturating_add(1);
        state.apply_capabilities.insert(id.clone(), capability);
        state.active_checkpoints.insert(reference);
        let worker_inner = Arc::clone(&self.inner);
        let worker_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("immich-web-apply".to_owned())
            .spawn(move || super::worker::run(&worker_inner, &worker_id));
        let Ok(handle) = spawned else {
            state.jobs.remove(&id);
            state.order.retain(|candidate| candidate != &id);
            state.queued = state.queued.saturating_sub(1);
            state.active_checkpoints.remove(&reference);
            state.apply_capabilities.remove(&id);
            return Err(AdmissionError::Unavailable);
        };
        state.handles.push(super::OwnedWorker { handle });
        drop(state);
        Ok(id)
    }
}
