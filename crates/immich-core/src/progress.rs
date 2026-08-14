use serde::{Deserialize, Serialize};

/// Pipeline stage represented by a progress event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    /// Filesystem entry discovery.
    Discovery,
    /// Bounded content hashing.
    ContentIdentity,
    /// Deterministic metadata and live-photo matching.
    Reconciliation,
    /// Immutable plan finalization.
    Complete,
}

/// Stable, low-cardinality progress event that omits media paths and metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressEvent {
    /// Progress event schema version.
    pub schema_version: u32,
    /// Monotonic event sequence within one scan.
    pub sequence: u64,
    /// Current read-only stage.
    pub stage: ProgressStage,
    /// Assets observed so far.
    pub assets_observed: u64,
    /// Source content bytes streamed so far.
    pub bytes_read: u64,
}
