#![forbid(unsafe_code)]
//! Deterministic, bounded and read-only source adapters.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use immich_rs_core::{
    Cancellation, LivePhotoMember, MediaKind, MetadataCandidate, MetadataKind, NormalizedMetadata,
    NormalizedPlan, PROGRESS_EVENT_SCHEMA_VERSION, PlanDiagnostic, ProgressEvent, ProgressStage,
    RuleEvidence,
};

mod apple_archive;
mod apple_photos;
mod discovery;
mod google_takeout;
mod identity;
mod reconcile;
mod scan;
mod takeout_archive;
mod takeout_metadata;
mod takeout_reconcile;

pub use apple_photos::{AlbumMode, ApplePhotosScanConfig, scan_apple_photos_inputs};
pub use google_takeout::{scan_google_takeout, scan_google_takeout_inputs};
pub use scan::{FolderScanConfig, NoProgress, ProgressObserver, ScanError, TakeoutScanConfig};

const MAX_DIAGNOSTIC_PATHS: usize = 8;

#[derive(Clone, Debug)]
struct DiscoveredMedia {
    native_path: PathBuf,
    relative_path: String,
    kind: MediaKind,
    byte_len: u64,
    content_sha256: String,
    metadata: Vec<MetadataCandidate>,
    normalized_metadata: Option<NormalizedMetadata>,
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
    takeout_document: Option<takeout_metadata::TakeoutDocument>,
    takeout_parse_error: Option<takeout_metadata::ParseError>,
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

#[derive(Clone, Copy)]
pub(crate) struct ScanStrategy {
    pub source_kind: immich_rs_core::SourceKind,
    pub schema_version: u32,
    pub reconcile_state: fn(&mut ScanState),
    pub skip_path: fn(&str) -> Option<String>,
}

const fn keep_all_paths(_path: &str) -> Option<String> {
    None
}

const FOLDER_SCAN: ScanStrategy = ScanStrategy {
    source_kind: immich_rs_core::SourceKind::Folder,
    schema_version: immich_rs_core::NORMALIZED_PLAN_SCHEMA_VERSION,
    reconcile_state: reconcile::reconcile,
    skip_path: keep_all_paths,
};

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
    scan_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
        FOLDER_SCAN,
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
    scan_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
        FOLDER_SCAN,
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
    scan_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        before_read,
        FOLDER_SCAN,
    )
    .map(|resolved| resolved.plan)
}

pub(crate) fn scan_resolved_internal(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
    strategy: ScanStrategy,
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
        strategy.skip_path,
    )?;
    discovery::check_cancelled(cancellation)?;
    (strategy.reconcile_state)(&mut state);
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
    let plan = reconcile::finalize_plan(
        strategy.schema_version,
        strategy.source_kind,
        source_label,
        config,
        state,
    );
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
mod apple_archive_tests;
#[cfg(test)]
mod apple_tests;
#[cfg(test)]
mod filesystem_tests;
#[cfg(test)]
mod resolved_tests;
#[cfg(test)]
mod takeout_archive_tests;
#[cfg(test)]
mod takeout_reconcile_tests;
#[cfg(test)]
mod takeout_tests;
#[cfg(test)]
mod tests;
