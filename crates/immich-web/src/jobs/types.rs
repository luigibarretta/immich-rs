use std::collections::VecDeque;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use immich_rs_application::{CancellationToken, ProgressStage};
use tokio::sync::broadcast;

use crate::state_store::{ArtifactRef, HistoryKind, PlanArtifact, ReceiptRef};

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
    pub artifact: Option<PlanArtifact>,
    pub receipt: Option<ReceiptRef>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum JobKind {
    Scan,
    Plan { server_id: String },
    DryRun { reference: ArtifactRef },
    Apply { reference: ArtifactRef },
}

impl JobKind {
    pub(super) const fn history_kind(&self) -> HistoryKind {
        match self {
            Self::Scan => HistoryKind::FolderScan,
            Self::Plan { .. } => HistoryKind::FolderPlan,
            Self::DryRun { .. } => HistoryKind::FolderDryRun,
            Self::Apply { .. } => HistoryKind::FolderApply,
        }
    }
}

pub(super) struct StoredJob {
    pub id: String,
    pub owner: [u8; 32],
    pub source_id: String,
    pub source_label: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub cancellation_requested: bool,
    pub cancellation: CancellationToken,
    pub progress: JobProgress,
    pub summary: Option<JobSummary>,
    pub artifact: Option<PlanArtifact>,
    pub receipt: Option<ReceiptRef>,
    pub events: VecDeque<JobEvent>,
    pub event_sender: broadcast::Sender<JobEvent>,
    pub next_event_sequence: u64,
}

impl StoredJob {
    pub(super) fn new(
        id: String,
        owner: [u8; 32],
        source_id: String,
        source_label: String,
        kind: JobKind,
        event_sender: broadcast::Sender<JobEvent>,
        replay_events: usize,
    ) -> Self {
        Self {
            id,
            owner,
            source_id,
            source_label,
            kind,
            status: JobStatus::Queued,
            cancellation_requested: false,
            cancellation: CancellationToken::default(),
            progress: JobProgress::default(),
            summary: None,
            artifact: None,
            receipt: None,
            events: VecDeque::with_capacity(replay_events),
            event_sender,
            next_event_sequence: 0,
        }
    }
}

pub(super) fn random_job_id() -> Result<String, ()> {
    let mut bytes = [0_u8; JOB_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    let id = URL_SAFE_NO_PAD.encode(bytes);
    bytes.fill(0);
    Ok(id)
}
