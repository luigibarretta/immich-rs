use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use super::{JobEvent, JobManager, JobStatus, JobsInner};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscribeError {
    NotFound,
    InvalidCursor,
    ReplayExhausted,
    SessionCapacity,
    ProcessCapacity,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionDelivery {
    Event(JobEvent),
    Heartbeat,
    ReplayExhausted,
}

pub struct JobSubscription {
    replay: VecDeque<JobEvent>,
    receiver: broadcast::Receiver<JobEvent>,
    close_after_replay: bool,
    closed: bool,
    _lease: SubscriberLease,
}

impl JobSubscription {
    pub async fn next(&mut self, heartbeat: Duration) -> Option<SubscriptionDelivery> {
        if self.closed {
            return None;
        }
        if let Some(event) = self.replay.pop_front() {
            if event.status.terminal() {
                self.closed = true;
            }
            return Some(SubscriptionDelivery::Event(event));
        }
        if self.close_after_replay {
            self.closed = true;
            return None;
        }
        match tokio::time::timeout(heartbeat, self.receiver.recv()).await {
            Err(_) => Some(SubscriptionDelivery::Heartbeat),
            Ok(Ok(event)) => {
                if event.status.terminal() {
                    self.closed = true;
                }
                Some(SubscriptionDelivery::Event(event))
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                self.closed = true;
                Some(SubscriptionDelivery::ReplayExhausted)
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                self.closed = true;
                None
            }
        }
    }
}

impl JobManager {
    pub fn subscribe(
        &self,
        id: &str,
        owner: [u8; 32],
        after: Option<u64>,
    ) -> Result<JobSubscription, SubscribeError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SubscribeError::Unavailable)?;
        let job = state
            .jobs
            .get(id)
            .filter(|job| job.owner == owner)
            .ok_or(SubscribeError::NotFound)?;
        let replay = select_replay(&job.events, after)?;
        let receiver = job.event_sender.subscribe();
        let close_after_replay = job.status == JobStatus::Completed
            || job.status == JobStatus::Failed
            || job.status == JobStatus::Cancelled;
        let owner_count = state.subscribers_by_owner.get(&owner).copied().unwrap_or(0);
        if owner_count >= self.inner.limits.sse_subscribers_per_session {
            return Err(SubscribeError::SessionCapacity);
        }
        if state.subscribers >= self.inner.limits.sse_subscribers_per_process {
            return Err(SubscribeError::ProcessCapacity);
        }
        state.subscribers = state.subscribers.saturating_add(1);
        state
            .subscribers_by_owner
            .insert(owner, owner_count.saturating_add(1));
        drop(state);
        Ok(JobSubscription {
            replay,
            receiver,
            close_after_replay,
            closed: false,
            _lease: SubscriberLease {
                inner: Arc::clone(&self.inner),
                owner,
            },
        })
    }
}

fn select_replay(
    events: &VecDeque<JobEvent>,
    after: Option<u64>,
) -> Result<VecDeque<JobEvent>, SubscribeError> {
    let Some(after) = after else {
        return Ok(events.clone());
    };
    let Some(first) = events.front().map(|event| event.sequence) else {
        return Err(SubscribeError::Unavailable);
    };
    let Some(last) = events.back().map(|event| event.sequence) else {
        return Err(SubscribeError::Unavailable);
    };
    if after > last {
        return Err(SubscribeError::InvalidCursor);
    }
    if after.saturating_add(1) < first {
        return Err(SubscribeError::ReplayExhausted);
    }
    Ok(events
        .iter()
        .filter(|event| event.sequence > after)
        .copied()
        .collect())
}

struct SubscriberLease {
    inner: Arc<JobsInner>,
    owner: [u8; 32],
}

impl Drop for SubscriberLease {
    fn drop(&mut self) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        state.subscribers = state.subscribers.saturating_sub(1);
        if let Some(count) = state.subscribers_by_owner.get_mut(&self.owner) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.subscribers_by_owner.remove(&self.owner);
            }
        }
    }
}
