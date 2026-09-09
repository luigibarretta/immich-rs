use immich_rs_core::ProgressEvent;
use immich_rs_sources::ProgressObserver;
use serde::{Deserialize, Serialize};

/// Schema version for frontend-neutral workflow progress.
pub const APPLICATION_PROGRESS_SCHEMA_VERSION: u32 = 1;

/// Versioned workflow event that preserves the nested scan-event contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplicationProgressEvent {
    /// Existing read-only scanner progress, without reinterpreting its fields.
    Scan {
        /// Application progress schema version.
        schema_version: u32,
        /// Original versioned scan event.
        event: ProgressEvent,
    },
    /// Reserved execution effect progress for executor-owned workflows.
    Execution {
        /// Application progress schema version.
        schema_version: u32,
        /// Monotonic sequence within one execution.
        sequence: u64,
        /// Durable logical effects completed so far.
        completed_effects: u64,
        /// Exact maximum logical effects from the immutable plan.
        maximum_effects: u64,
    },
}

/// Synchronous low-cardinality observer used by either frontend.
pub trait ApplicationProgressObserver {
    /// Receive one bounded versioned workflow event.
    fn observe(&mut self, event: ApplicationProgressEvent);
}

/// Observer for callers that do not request progress.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApplicationNoProgress;

impl ApplicationProgressObserver for ApplicationNoProgress {
    fn observe(&mut self, _event: ApplicationProgressEvent) {}
}

pub struct ScanProgressAdapter<'a, Observer> {
    observer: &'a mut Observer,
}

impl<'a, Observer> ScanProgressAdapter<'a, Observer> {
    pub const fn new(observer: &'a mut Observer) -> Self {
        Self { observer }
    }
}

impl<Observer: ApplicationProgressObserver> ProgressObserver for ScanProgressAdapter<'_, Observer> {
    fn observe(&mut self, event: ProgressEvent) {
        self.observer.observe(ApplicationProgressEvent::Scan {
            schema_version: APPLICATION_PROGRESS_SCHEMA_VERSION,
            event,
        });
    }
}
