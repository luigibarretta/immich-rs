#![forbid(unsafe_code)]
//! Deterministic, bounded and read-only source adapters.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use immich_rs_core::{
    Cancellation, LivePhotoMember, MediaKind, MetadataCandidate, MetadataKind, NormalizedPlan,
    PROGRESS_EVENT_SCHEMA_VERSION, PlanDiagnostic, PlanValidationError, ProgressEvent,
    ProgressStage, RuleEvidence,
};

mod discovery;
mod identity;
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
    native_path: PathBuf,
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
    native_path: PathBuf,
    relative_path: String,
    kind: MetadataKind,
    byte_len: u64,
    content_sha256: String,
}

/// One normalized plan paired with ephemeral native source path resolution.
#[derive(Clone, Debug)]
pub struct ResolvedFolderPlan {
    /// Source-neutral, serializable plan.
    pub plan: NormalizedPlan,
    files: BTreeMap<String, ResolvedSourceFile>,
}

impl ResolvedFolderPlan {
    /// Resolve one NFC portable path to the native path observed by the scan.
    #[must_use]
    pub fn native_path(&self, portable_path: &str) -> Option<&Path> {
        self.files
            .get(portable_path)
            .map(|file| file.native_path.as_path())
    }

    /// Resolve content facts captured during the same bounded scan.
    #[must_use]
    pub fn source_file(&self, portable_path: &str) -> Option<&ResolvedSourceFile> {
        self.files.get(portable_path)
    }
}

/// Ephemeral native resolution and content identity for one scanned file.
#[derive(Clone, Debug)]
pub struct ResolvedSourceFile {
    native_path: PathBuf,
    byte_len: u64,
    content_sha256: String,
}

impl ResolvedSourceFile {
    /// Native path observed by discovery.
    #[must_use]
    pub fn native_path(&self) -> &Path {
        &self.native_path
    }

    /// Exact length observed during the streaming identity read.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// SHA-256 observed during the streaming identity read.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

struct RegularFile<'a> {
    path: &'a Path,
    relative_path: String,
    metadata: &'a Metadata,
}

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
    scan_folder_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
    )
    .map(|resolved| resolved.plan)
}

/// Scan once and retain bounded native paths for a subsequent local apply plan.
pub fn scan_folder_resolved(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    scan_folder_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
    )
}

#[cfg(test)]
fn scan_folder_internal(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
) -> Result<NormalizedPlan, ScanError> {
    scan_folder_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        before_read,
    )
    .map(|resolved| resolved.plan)
}

fn scan_folder_resolved_internal(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
) -> Result<ResolvedFolderPlan, ScanError> {
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
    let files = state
        .media
        .iter()
        .map(|media| {
            (
                media.relative_path.clone(),
                ResolvedSourceFile {
                    native_path: media.native_path.clone(),
                    byte_len: media.byte_len,
                    content_sha256: media.content_sha256.clone(),
                },
            )
        })
        .chain(state.sidecars.iter().map(|sidecar| {
            (
                sidecar.relative_path.clone(),
                ResolvedSourceFile {
                    native_path: sidecar.native_path.clone(),
                    byte_len: sidecar.byte_len,
                    content_sha256: sidecar.content_sha256.clone(),
                },
            )
        }))
        .collect::<BTreeMap<_, _>>();
    let plan = reconcile::finalize_plan(source_label, config, state);
    plan.validate()?;
    observer.observe(ProgressEvent {
        schema_version: PROGRESS_EVENT_SCHEMA_VERSION,
        sequence: complete_sequence,
        stage: ProgressStage::Complete,
        assets_observed: plan.summary.assets,
        bytes_read: plan.summary.bytes_read,
    });
    Ok(ResolvedFolderPlan { plan, files })
}

#[cfg(test)]
mod resolved_tests;
#[cfg(test)]
mod tests;
