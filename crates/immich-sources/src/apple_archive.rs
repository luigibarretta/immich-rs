use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::path::{Path, PathBuf};

use immich_rs_core::{
    Cancellation, MediaKind, MetadataKind, NORMALIZED_PLAN_SCHEMA_VERSION_V3,
    NORMALIZED_PLAN_SCHEMA_VERSION_V4, ProgressStage, RuleEvidence, SourceKind, rule_id,
};
use zip::ZipArchive;

use crate::apple_photos::{ApplePhotosScanConfig, export_noise_path, finish_plan};
use crate::archive_support::{portable_entry_path, stream_entry, validate_entry, zip_unix_ms};
use crate::discovery::{media_kind, metadata_kind, validate_source_label};
use crate::reconcile::{diagnostic, finalize_plan};
use crate::{
    DiscoveredMedia, DiscoveredSidecar, ProgressObserver, ResolvedFolderPlan, ScanError, ScanState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Media(MediaKind),
    Sidecar(MetadataKind),
}

#[derive(Clone, Copy)]
enum ArchiveFlavor {
    Apple,
    Picasa,
}

impl ArchiveFlavor {
    const fn source(self) -> (u32, SourceKind) {
        match self {
            Self::Apple => (NORMALIZED_PLAN_SCHEMA_VERSION_V3, SourceKind::ApplePhotos),
            Self::Picasa => (NORMALIZED_PLAN_SCHEMA_VERSION_V4, SourceKind::Picasa),
        }
    }

    const fn rules(self) -> (&'static str, &'static str) {
        match self {
            Self::Apple => (
                rule_id::APPLE_ARCHIVE_DUPLICATE,
                rule_id::APPLE_ARCHIVE_CONFLICT,
            ),
            Self::Picasa => (
                rule_id::PICASA_ARCHIVE_DUPLICATE,
                rule_id::PICASA_ARCHIVE_CONFLICT,
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntryIdentity {
    kind: EntryKind,
    byte_len: u64,
    content_sha256: String,
    content_sha1_base64: String,
    modified_at_unix_ms: Option<i64>,
}

pub fn scan_archives_resolved(
    inputs: &[PathBuf],
    source_label: &str,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    scan_archives(
        inputs,
        source_label,
        config,
        cancellation,
        observer,
        ArchiveFlavor::Apple,
    )
}

pub fn scan_picasa_archives_resolved(
    inputs: &[PathBuf],
    source_label: &str,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    scan_archives(
        inputs,
        source_label,
        config,
        cancellation,
        observer,
        ArchiveFlavor::Picasa,
    )
}

fn scan_archives(
    inputs: &[PathBuf],
    source_label: &str,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    flavor: ArchiveFlavor,
) -> Result<ResolvedFolderPlan, ScanError> {
    config.validate()?;
    validate_source_label(source_label)?;
    validate_inputs(inputs, config.max_archives)?;
    let mut state = ScanState::new();
    let mut seen = BTreeMap::<String, Option<EntryIdentity>>::new();
    for input in inputs {
        scan_archive(
            input,
            config,
            cancellation,
            observer,
            &mut state,
            &mut seen,
            flavor,
        )?;
    }
    crate::reconcile::reconcile(&mut state);
    state.progress(ProgressStage::Reconciliation, observer);
    let sequence = state.event_sequence.saturating_add(1);
    let files = crate::resolved_files(&state);
    let (schema_version, source_kind) = flavor.source();
    let plan = finalize_plan(
        schema_version,
        source_kind,
        source_label,
        &config.scan,
        state,
    );
    let plan = match flavor {
        ArchiveFlavor::Apple => finish_plan(plan, config)?,
        ArchiveFlavor::Picasa => plan,
    };
    crate::finish_resolved(plan, files, sequence, observer)
}

fn validate_inputs(inputs: &[PathBuf], max_archives: usize) -> Result<(), ScanError> {
    if inputs.is_empty() || inputs.len() > max_archives {
        return Err(ScanError::LimitExceeded("max_archives"));
    }
    for input in inputs {
        let metadata = std::fs::symlink_metadata(input)
            .map_err(|_| ScanError::InvalidArchive("archive is not readable"))?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || input
                .extension()
                .and_then(|value| value.to_str())
                .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
        {
            return Err(ScanError::InvalidArchive(
                "inputs must be non-symlink ZIP files",
            ));
        }
    }
    Ok(())
}

fn scan_archive(
    path: &Path,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    state: &mut ScanState,
    seen: &mut BTreeMap<String, Option<EntryIdentity>>,
    flavor: ArchiveFlavor,
) -> Result<(), ScanError> {
    check_cancelled(cancellation)?;
    let file = File::open(path).map_err(|_| ScanError::InvalidArchive("cannot open archive"))?;
    let before = file
        .metadata()
        .map_err(|_| ScanError::InvalidArchive("cannot inspect archive"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|_| ScanError::InvalidArchive("invalid ZIP directory"))?;
    if state.entries_seen.saturating_add(archive.len()) > config.scan.max_entries {
        return Err(ScanError::LimitExceeded("max_entries"));
    }
    for index in 0..archive.len() {
        check_cancelled(cancellation)?;
        state.entries_seen = state.entries_seen.saturating_add(1);
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ScanError::InvalidArchive("cannot open ZIP entry"))?;
        let relative_path = portable_entry_path(&entry, config.scan.max_path_bytes)?;
        if matches!(flavor, ArchiveFlavor::Apple)
            && let Some(diagnostic_path) = export_noise_path(&relative_path)
        {
            state.warnings.push(diagnostic(
                rule_id::APPLE_EXPORT_NOISE,
                "known_apple_export_noise",
                vec![diagnostic_path],
            ));
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let kind = match (media_kind(&relative_path), metadata_kind(&relative_path)) {
            (Some(kind), _) => EntryKind::Media(kind),
            (_, Some(kind)) => EntryKind::Sidecar(kind),
            _ => continue,
        };
        validate_entry(
            &entry,
            config.max_archive_entry_bytes,
            config.max_compression_ratio,
            config.compression_ratio_grace_bytes,
        )?;
        let modified_at_unix_ms = zip_unix_ms(entry.last_modified());
        let (byte_len, content_sha256, content_sha1_base64) =
            stream_entry(&mut entry, config.scan.buffer_bytes, cancellation)?;
        state.bytes_read = state.bytes_read.saturating_add(byte_len);
        merge_entry(
            path,
            index,
            relative_path,
            EntryIdentity {
                kind,
                byte_len,
                content_sha256,
                content_sha1_base64,
                modified_at_unix_ms,
            },
            state,
            seen,
            flavor,
        );
        state.progress(ProgressStage::ContentIdentity, observer);
    }
    let file = archive.into_inner();
    let after = file
        .metadata()
        .map_err(|_| ScanError::InvalidArchive("cannot recheck archive"))?;
    if metadata_changed(&before, &after) {
        return Err(ScanError::InvalidArchive("archive changed during scan"));
    }
    Ok(())
}

fn merge_entry(
    archive_path: &Path,
    archive_index: usize,
    relative_path: String,
    identity: EntryIdentity,
    state: &mut ScanState,
    seen: &mut BTreeMap<String, Option<EntryIdentity>>,
    flavor: ArchiveFlavor,
) {
    let (duplicate_rule, conflict_rule) = flavor.rules();
    if let Some(previous) = seen.get_mut(&relative_path) {
        if previous.as_ref() == Some(&identity) {
            state.warnings.push(diagnostic(
                duplicate_rule,
                "identical_archive_entry",
                vec![relative_path],
            ));
        } else {
            *previous = None;
            state
                .media
                .retain(|item| item.relative_path != relative_path);
            state
                .sidecars
                .retain(|item| item.relative_path != relative_path);
            state.errors.push(diagnostic(
                conflict_rule,
                "conflicting_archive_entry",
                vec![relative_path],
            ));
        }
        return;
    }
    seen.insert(relative_path.clone(), Some(identity.clone()));
    match identity.kind {
        EntryKind::Media(kind) => state.media.push(DiscoveredMedia {
            native_path: archive_path.to_path_buf(),
            archive_index: Some(archive_index),
            relative_path,
            kind,
            byte_len: identity.byte_len,
            content_sha256: identity.content_sha256,
            content_sha1_base64: identity.content_sha1_base64,
            created_at_unix_ms: identity.modified_at_unix_ms,
            modified_at_unix_ms: identity.modified_at_unix_ms,
            metadata: Vec::new(),
            normalized_metadata: None,
            live_photo: None,
            evidence: vec![RuleEvidence {
                rule_id: rule_id::REGULAR_MEDIA.to_owned(),
                outcome: "streamed_content_identity".to_owned(),
            }],
        }),
        EntryKind::Sidecar(kind) => state.sidecars.push(DiscoveredSidecar {
            native_path: archive_path.to_path_buf(),
            archive_index: Some(archive_index),
            relative_path,
            kind,
            byte_len: identity.byte_len,
            content_sha256: identity.content_sha256,
            content_sha1_base64: identity.content_sha1_base64,
            created_at_unix_ms: identity.modified_at_unix_ms,
            modified_at_unix_ms: identity.modified_at_unix_ms,
            takeout_document: None,
            takeout_parse_error: None,
        }),
    }
}

fn metadata_changed(before: &Metadata, after: &Metadata) -> bool {
    before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.created().ok() != after.created().ok()
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ScanError> {
    if cancellation.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}
