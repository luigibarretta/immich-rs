use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use immich_rs_core::{
    Cancellation, MediaKind, MetadataKind, NORMALIZED_PLAN_SCHEMA_VERSION_V3, NormalizedPlan,
    PROGRESS_EVENT_SCHEMA_VERSION, ProgressEvent, ProgressStage, RuleEvidence, SourceKind, rule_id,
};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization as _;
use zip::read::ZipFile;
use zip::{CompressionMethod, ZipArchive};

use crate::apple_photos::{ApplePhotosScanConfig, export_noise_path, finish_plan};
use crate::discovery::{media_kind, metadata_kind, validate_source_label};
use crate::reconcile::{diagnostic, finalize_plan};
use crate::{DiscoveredMedia, DiscoveredSidecar, ProgressObserver, ScanError, ScanState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Media(MediaKind),
    Sidecar(MetadataKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntryIdentity {
    kind: EntryKind,
    byte_len: u64,
    content_sha256: String,
}

pub fn scan_archives(
    inputs: &[PathBuf],
    source_label: &str,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    config.validate()?;
    validate_source_label(source_label)?;
    validate_inputs(inputs, config.max_archives)?;
    let mut state = ScanState::new();
    let mut seen = BTreeMap::<String, Option<EntryIdentity>>::new();
    for input in inputs {
        scan_archive(input, config, cancellation, observer, &mut state, &mut seen)?;
    }
    crate::reconcile::reconcile(&mut state);
    state.progress(ProgressStage::Reconciliation, observer);
    let sequence = state.event_sequence.saturating_add(1);
    let plan = finalize_plan(
        NORMALIZED_PLAN_SCHEMA_VERSION_V3,
        SourceKind::ApplePhotos,
        source_label,
        &config.scan,
        state,
    );
    let plan = finish_plan(plan, config)?;
    observer.observe(ProgressEvent {
        schema_version: PROGRESS_EVENT_SCHEMA_VERSION,
        sequence,
        stage: ProgressStage::Complete,
        assets_observed: plan.summary.assets,
        bytes_read: plan.summary.bytes_read,
    });
    Ok(plan)
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
        if let Some(diagnostic_path) = export_noise_path(&relative_path) {
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
        validate_entry(&entry, config)?;
        let (byte_len, content_sha256) =
            stream_entry(&mut entry, config.scan.buffer_bytes, cancellation)?;
        state.bytes_read = state.bytes_read.saturating_add(byte_len);
        merge_entry(
            path,
            relative_path,
            EntryIdentity {
                kind,
                byte_len,
                content_sha256,
            },
            state,
            seen,
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

fn portable_entry_path<R: Read>(
    entry: &ZipFile<'_, R>,
    max_path_bytes: usize,
) -> Result<String, ScanError> {
    if entry.name().contains(['\\', '\0'])
        || entry
            .name()
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(ScanError::InvalidArchive("unsafe ZIP entry path"));
    }
    let enclosed = entry
        .enclosed_name()
        .ok_or(ScanError::InvalidArchive("unsafe ZIP entry path"))?;
    let mut components = Vec::new();
    for component in enclosed.components() {
        let Component::Normal(value) = component else {
            return Err(ScanError::InvalidArchive("unsafe ZIP entry path"));
        };
        let value = value
            .to_str()
            .ok_or(ScanError::InvalidArchive("non-Unicode ZIP entry path"))?;
        components.push(value.nfc().collect::<String>());
    }
    let path = components.join("/");
    if path.is_empty() || path.len() > max_path_bytes {
        return Err(ScanError::LimitExceeded("max_path_bytes"));
    }
    Ok(path)
}

fn validate_entry<R: Read>(
    entry: &ZipFile<'_, R>,
    config: &ApplePhotosScanConfig,
) -> Result<(), ScanError> {
    if entry.encrypted() || entry.is_symlink() || !entry.is_file() {
        return Err(ScanError::InvalidArchive(
            "encrypted or non-regular ZIP entry",
        ));
    }
    if !matches!(
        entry.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(ScanError::InvalidArchive(
            "unsupported ZIP compression method",
        ));
    }
    if entry.size() > config.max_archive_entry_bytes {
        return Err(ScanError::LimitExceeded("max_archive_entry_bytes"));
    }
    if entry.size() > config.compression_ratio_grace_bytes
        && (entry.compressed_size() == 0
            || entry.size() / entry.compressed_size() > config.max_compression_ratio)
    {
        return Err(ScanError::InvalidArchive(
            "ZIP entry compression ratio exceeded",
        ));
    }
    Ok(())
}

fn stream_entry<R: Read>(
    entry: &mut ZipFile<'_, R>,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<(u64, String), ScanError> {
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut digest = Sha256::new();
    let mut byte_len = 0_u64;
    loop {
        check_cancelled(cancellation)?;
        let count = entry
            .read(&mut buffer)
            .map_err(|_| ScanError::InvalidArchive("cannot read or verify ZIP entry"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        byte_len = byte_len.saturating_add(count as u64);
    }
    if byte_len != entry.size() {
        return Err(ScanError::InvalidArchive("ZIP entry size mismatch"));
    }
    Ok((byte_len, format!("{:x}", digest.finalize())))
}

fn merge_entry(
    archive_path: &Path,
    relative_path: String,
    identity: EntryIdentity,
    state: &mut ScanState,
    seen: &mut BTreeMap<String, Option<EntryIdentity>>,
) {
    if let Some(previous) = seen.get_mut(&relative_path) {
        if previous.as_ref() == Some(&identity) {
            state.warnings.push(diagnostic(
                rule_id::APPLE_ARCHIVE_DUPLICATE,
                "identical_apple_archive_entry",
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
                rule_id::APPLE_ARCHIVE_CONFLICT,
                "conflicting_apple_archive_entry",
                vec![relative_path],
            ));
        }
        return;
    }
    seen.insert(relative_path.clone(), Some(identity.clone()));
    match identity.kind {
        EntryKind::Media(kind) => state.media.push(DiscoveredMedia {
            native_path: archive_path.to_path_buf(),
            relative_path,
            kind,
            byte_len: identity.byte_len,
            content_sha256: identity.content_sha256,
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
            relative_path,
            kind,
            byte_len: identity.byte_len,
            content_sha256: identity.content_sha256,
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
