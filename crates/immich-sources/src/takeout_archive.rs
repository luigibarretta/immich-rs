use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::{
    Cancellation, MediaKind, MetadataKind, NORMALIZED_PLAN_SCHEMA_VERSION_V2, ProgressStage,
    RuleEvidence, SourceKind, rule_id,
};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization as _;
use zip::read::ZipFile;
use zip::{CompressionMethod, ZipArchive};

use crate::discovery::{media_kind, metadata_kind, validate_source_label};
use crate::reconcile::{diagnostic, finalize_plan};
use crate::takeout_metadata::{MAX_JSON_BYTES, ParseError, parse_bytes};
use crate::{
    DiscoveredMedia, DiscoveredSidecar, ProgressObserver, ResolvedFolderPlan, ScanError, ScanState,
    TakeoutScanConfig,
};

const TAKEOUT_PREFIX: &str = "Takeout/Google Photos/";

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
    content_sha1_base64: String,
}

pub fn scan_archives_resolved(
    inputs: &[PathBuf],
    source_label: &str,
    config: &TakeoutScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    config.validate()?;
    validate_source_label(source_label)?;
    validate_inputs(inputs, config.max_archives)?;
    let mut state = ScanState::new();
    let mut seen = BTreeMap::<String, Option<EntryIdentity>>::new();
    let mut layout_found = false;
    for input in inputs {
        scan_archive(
            input,
            config,
            cancellation,
            observer,
            &mut state,
            &mut seen,
            &mut layout_found,
        )?;
    }
    if !layout_found {
        return Err(ScanError::UnsupportedLayout(
            "archives must contain Takeout/Google Photos",
        ));
    }
    crate::takeout_reconcile::reconcile(&mut state);
    state.progress(ProgressStage::Reconciliation, observer);
    let sequence = state.event_sequence.saturating_add(1);
    let files = crate::resolved_files(&state);
    let plan = finalize_plan(
        NORMALIZED_PLAN_SCHEMA_VERSION_V2,
        SourceKind::GoogleTakeout,
        source_label,
        &config.scan,
        state,
    );
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

#[allow(clippy::too_many_arguments)]
fn scan_archive(
    path: &Path,
    config: &TakeoutScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    state: &mut ScanState,
    seen: &mut BTreeMap<String, Option<EntryIdentity>>,
    layout_found: &mut bool,
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
        if relative_path == TAKEOUT_PREFIX.trim_end_matches('/')
            || relative_path.starts_with(TAKEOUT_PREFIX)
        {
            *layout_found = true;
        }
        if entry.is_dir() || !relative_path.starts_with(TAKEOUT_PREFIX) {
            continue;
        }
        validate_entry(&entry, config)?;
        let kind = match (media_kind(&relative_path), metadata_kind(&relative_path)) {
            (Some(kind), _) => EntryKind::Media(kind),
            (_, Some(kind)) => EntryKind::Sidecar(kind),
            _ => continue,
        };
        let collect_json =
            kind == EntryKind::Sidecar(MetadataKind::Json) && entry.size() <= MAX_JSON_BYTES;
        let (byte_len, content_sha256, content_sha1_base64, bytes) = stream_entry(
            &mut entry,
            config.scan.buffer_bytes,
            collect_json,
            cancellation,
        )?;
        state.bytes_read = state.bytes_read.saturating_add(byte_len);
        let identity = EntryIdentity {
            kind,
            byte_len,
            content_sha256,
            content_sha1_base64,
        };
        merge_entry(path, index, relative_path, identity, &bytes, state, seen);
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
    config: &TakeoutScanConfig,
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
    collect: bool,
    cancellation: &impl Cancellation,
) -> Result<(u64, String, String, Vec<u8>), ScanError> {
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut collected = Vec::new();
    let mut digest = Sha256::new();
    let mut sha1 = Sha1::new();
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
        sha1.update(&buffer[..count]);
        if collect {
            collected.extend_from_slice(&buffer[..count]);
        }
        byte_len = byte_len.saturating_add(count as u64);
    }
    if byte_len != entry.size() {
        return Err(ScanError::InvalidArchive("ZIP entry size mismatch"));
    }
    Ok((
        byte_len,
        format!("{:x}", digest.finalize()),
        STANDARD.encode(sha1.finalize()),
        collected,
    ))
}

fn merge_entry(
    archive_path: &Path,
    archive_index: usize,
    relative_path: String,
    identity: EntryIdentity,
    bytes: &[u8],
    state: &mut ScanState,
    seen: &mut BTreeMap<String, Option<EntryIdentity>>,
) {
    if let Some(previous) = seen.get_mut(&relative_path) {
        if previous.as_ref() == Some(&identity) {
            state.warnings.push(diagnostic(
                rule_id::TAKEOUT_ARCHIVE_DUPLICATE,
                "identical_takeout_archive_entry",
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
                rule_id::TAKEOUT_ARCHIVE_CONFLICT,
                "conflicting_takeout_archive_entry",
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
            created_at_unix_ms: None,
            modified_at_unix_ms: None,
            metadata: Vec::new(),
            normalized_metadata: None,
            live_photo: None,
            evidence: vec![RuleEvidence {
                rule_id: rule_id::REGULAR_MEDIA.to_owned(),
                outcome: "streamed_content_identity".to_owned(),
            }],
        }),
        EntryKind::Sidecar(kind) => {
            let (takeout_document, takeout_parse_error) = if kind != MetadataKind::Json {
                (None, None)
            } else if identity.byte_len > MAX_JSON_BYTES {
                (None, Some(ParseError::Oversized))
            } else {
                match parse_bytes(bytes) {
                    Ok(document) => (Some(document), None),
                    Err(error) => (None, Some(error)),
                }
            };
            state.sidecars.push(DiscoveredSidecar {
                native_path: archive_path.to_path_buf(),
                archive_index: Some(archive_index),
                relative_path,
                kind,
                byte_len: identity.byte_len,
                content_sha256: identity.content_sha256,
                content_sha1_base64: identity.content_sha1_base64,
                created_at_unix_ms: None,
                modified_at_unix_ms: None,
                takeout_document,
                takeout_parse_error,
            });
        }
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
