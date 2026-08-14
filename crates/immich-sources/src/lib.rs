#![forbid(unsafe_code)]
//! Deterministic, bounded and read-only source adapters.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::Metadata;
use std::path::Path;

use immich_rs_core::{
    Cancellation, LivePhotoMember, MediaKind, MetadataCandidate, MetadataKind, NormalizedPlan,
    PROGRESS_EVENT_SCHEMA_VERSION, PlanDiagnostic, PlanValidationError, ProgressEvent,
    ProgressStage, RuleEvidence,
};

mod discovery;
mod reconcile;

const MIN_BUFFER_BYTES: usize = 4 * 1024;
const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIAGNOSTIC_PATHS: usize = 8;

/// Explicit memory and portability limits for one folder scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderScanConfig {
    /// Read buffer used for one media file at a time.
    pub buffer_bytes: usize,
    /// Maximum total entries retained by discovery and reconciliation.
    pub max_entries: usize,
    /// Maximum entries accepted in one directory before failing closed.
    pub max_directory_entries: usize,
    /// Maximum UTF-8 bytes in one portable relative path.
    pub max_path_bytes: usize,
    /// Case policy used for the portable source description.
    pub case_sensitive: bool,
}

impl Default for FolderScanConfig {
    fn default() -> Self {
        Self {
            buffer_bytes: 64 * 1024,
            max_entries: 100_000,
            max_directory_entries: 10_000,
            max_path_bytes: 4_096,
            case_sensitive: true,
        }
    }
}

impl FolderScanConfig {
    fn validate(&self) -> Result<(), ScanError> {
        if !(MIN_BUFFER_BYTES..=MAX_BUFFER_BYTES).contains(&self.buffer_bytes) {
            return Err(ScanError::InvalidConfiguration(
                "buffer_bytes must be in 4096..=4194304",
            ));
        }
        if self.max_entries == 0 || self.max_directory_entries == 0 {
            return Err(ScanError::InvalidConfiguration(
                "entry limits must be greater than zero",
            ));
        }
        if !(64..=65_536).contains(&self.max_path_bytes) {
            return Err(ScanError::InvalidConfiguration(
                "max_path_bytes must be in 64..=65536",
            ));
        }
        Ok(())
    }
}

/// Low-cardinality observer called synchronously by the scanner.
pub trait ProgressObserver {
    /// Receive one monotonic read-only progress event.
    fn observe(&mut self, event: ProgressEvent);
}

/// Observer used when callers do not request progress events.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProgress;

impl ProgressObserver for NoProgress {
    fn observe(&mut self, _event: ProgressEvent) {}
}

/// Fail-closed folder scan failure.
#[derive(Debug)]
pub enum ScanError {
    /// A configured resource limit is invalid.
    InvalidConfiguration(&'static str),
    /// The source root is not a readable real directory.
    InvalidRoot,
    /// A deterministic memory or path limit was reached.
    LimitExceeded(&'static str),
    /// Cooperative cancellation stopped discovery or hashing.
    Cancelled,
    /// The produced plan violated a core invariant.
    InvalidPlan(PlanValidationError),
}

impl Display for ScanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid scan configuration: {message}")
            }
            Self::InvalidRoot => {
                formatter.write_str("source root must be a readable real directory")
            }
            Self::LimitExceeded(limit) => write!(formatter, "scan limit exceeded: {limit}"),
            Self::Cancelled => formatter.write_str("scan cancelled cleanly"),
            Self::InvalidPlan(error) => {
                write!(formatter, "scanner produced an invalid plan: {error}")
            }
        }
    }
}

impl Error for ScanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPlan(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PlanValidationError> for ScanError {
    fn from(error: PlanValidationError) -> Self {
        Self::InvalidPlan(error)
    }
}

#[derive(Clone, Debug)]
struct DiscoveredMedia {
    relative_path: String,
    kind: MediaKind,
    byte_len: u64,
    content_sha256: String,
    metadata: Vec<MetadataCandidate>,
    live_photo: Option<LivePhotoMember>,
    evidence: Vec<RuleEvidence>,
}

#[derive(Clone, Debug)]
struct DiscoveredSidecar {
    relative_path: String,
    kind: MetadataKind,
}

struct RegularFile<'a> {
    path: &'a Path,
    relative_path: String,
    metadata: &'a Metadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
    len: u64,
    modified_nanos: Option<u128>,
    platform_identity: PlatformIdentity,
}

impl FileSnapshot {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos()),
            platform_identity: platform_identity(metadata),
        }
    }
}

#[cfg(unix)]
type PlatformIdentity = (u64, u64);

#[cfg(not(unix))]
type PlatformIdentity = ();

#[cfg(unix)]
fn platform_identity(metadata: &Metadata) -> PlatformIdentity {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn platform_identity(_metadata: &Metadata) -> PlatformIdentity {}

#[derive(Debug)]
struct ScanState {
    media: Vec<DiscoveredMedia>,
    sidecars: Vec<DiscoveredSidecar>,
    warnings: Vec<PlanDiagnostic>,
    errors: Vec<PlanDiagnostic>,
    entries_seen: usize,
    bytes_read: u64,
    event_sequence: u64,
}

impl ScanState {
    const fn new() -> Self {
        Self {
            media: Vec::new(),
            sidecars: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            entries_seen: 0,
            bytes_read: 0,
            event_sequence: 0,
        }
    }

    fn progress(&mut self, stage: ProgressStage, observer: &mut impl ProgressObserver) {
        self.event_sequence = self.event_sequence.saturating_add(1);
        observer.observe(ProgressEvent {
            schema_version: PROGRESS_EVENT_SCHEMA_VERSION,
            sequence: self.event_sequence,
            stage,
            assets_observed: self.media.len() as u64,
            bytes_read: self.bytes_read,
        });
    }
}

/// Recursively scan a real folder and return only a normalized read-only plan.
pub fn scan_folder(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    scan_folder_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
    )
}

fn scan_folder_internal(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
) -> Result<NormalizedPlan, ScanError> {
    config.validate()?;
    discovery::validate_root_and_label(root, source_label)?;

    let mut state = ScanState::new();
    discovery::discover(
        root,
        config,
        cancellation,
        observer,
        before_read,
        &mut state,
    )?;
    discovery::check_cancelled(cancellation)?;
    reconcile::reconcile(&mut state);
    state.progress(ProgressStage::Reconciliation, observer);
    let complete_sequence = state.event_sequence.saturating_add(1);
    let plan = reconcile::finalize_plan(source_label, config, state);
    plan.validate()?;
    observer.observe(ProgressEvent {
        schema_version: PROGRESS_EVENT_SCHEMA_VERSION,
        sequence: complete_sequence,
        stage: ProgressStage::Complete,
        assets_observed: plan.summary.assets,
        bytes_read: plan.summary.bytes_read,
    });
    Ok(plan)
}

#[cfg(test)]
mod tests;
