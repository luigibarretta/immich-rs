use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use immich_rs_core::{Cancellation, MediaKind, MetadataKind, ProgressStage, RuleEvidence, rule_id};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization as _;

use crate::reconcile::diagnostic;
use crate::{
    DiscoveredMedia, DiscoveredSidecar, FileSnapshot, FolderScanConfig, ProgressObserver,
    RegularFile, ScanError, ScanState,
};

pub fn validate_root_and_label(root: &Path, source_label: &str) -> Result<(), ScanError> {
    if source_label.is_empty()
        || source_label.len() > 128
        || source_label.chars().any(char::is_control)
    {
        return Err(ScanError::InvalidConfiguration(
            "source label must be 1..=128 non-control characters",
        ));
    }
    let root_metadata = fs::symlink_metadata(root).map_err(|_| ScanError::InvalidRoot)?;
    if !root_metadata.file_type().is_dir() || root_metadata.file_type().is_symlink() {
        return Err(ScanError::InvalidRoot);
    }
    Ok(())
}

pub fn discover(
    root: &Path,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
    state: &mut ScanState,
) -> Result<(), ScanError> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        check_cancelled(cancellation)?;
        let entries = directory_entries(root, &directory, config, cancellation, state)?;
        for path in entries.into_iter().rev() {
            state.entries_seen = state.entries_seen.saturating_add(1);
            if state.entries_seen > config.max_entries {
                return Err(ScanError::LimitExceeded("max_entries"));
            }
            if let Some(child_directory) = process_entry(
                root,
                &path,
                config,
                cancellation,
                observer,
                before_read,
                state,
            )? {
                directories.push(child_directory);
            }
            state.progress(ProgressStage::Discovery, observer);
        }
    }
    Ok(())
}

fn directory_entries(
    root: &Path,
    directory: &Path,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    state: &mut ScanState,
) -> Result<Vec<PathBuf>, ScanError> {
    let Ok(read_directory) = fs::read_dir(directory) else {
        state.errors.push(diagnostic(
            rule_id::UNREADABLE_FILE,
            "unreadable_directory",
            vec![portable_directory(root, directory, config.max_path_bytes)],
        ));
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for entry_result in read_directory {
        check_cancelled(cancellation)?;
        if entries.len() >= config.max_directory_entries {
            return Err(ScanError::LimitExceeded("max_directory_entries"));
        }
        match entry_result {
            Ok(entry) => entries.push(entry.path()),
            Err(_) => state.errors.push(diagnostic(
                rule_id::UNREADABLE_FILE,
                "unreadable_directory_entry",
                vec![portable_directory(root, directory, config.max_path_bytes)],
            )),
        }
    }
    entries.sort_by_key(|path| portable_sort_key(root, path));
    Ok(entries)
}

fn process_entry(
    root: &Path,
    path: &Path,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
    state: &mut ScanState,
) -> Result<Option<PathBuf>, ScanError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        state.errors.push(diagnostic(
            rule_id::UNREADABLE_FILE,
            "unreadable_metadata",
            portable_path(root, path, config.max_path_bytes)
                .into_iter()
                .collect(),
        ));
        return Ok(None);
    };
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        return Ok(Some(path.to_path_buf()));
    }
    let relative_path = match portable_path(root, path, config.max_path_bytes) {
        Ok(relative_path) => relative_path,
        Err(PortablePathError::TooLong) => {
            state.errors.push(diagnostic(
                rule_id::PATH_LIMIT_EXCEEDED,
                "portable_path_too_long",
                vec!["<path-over-limit>".to_owned()],
            ));
            return Ok(None);
        }
        Err(PortablePathError::NonUnicode | PortablePathError::OutsideRoot) => {
            state.errors.push(diagnostic(
                rule_id::NON_UNICODE_PATH,
                "non_unicode_path",
                vec!["<non-unicode>".to_owned()],
            ));
            return Ok(None);
        }
    };
    if file_type.is_symlink() {
        state.warnings.push(diagnostic(
            rule_id::SYMLINK_SKIPPED,
            "symlink_not_followed",
            vec![relative_path],
        ));
    } else if file_type.is_file() {
        process_regular_file(
            RegularFile {
                path,
                relative_path,
                metadata: &metadata,
            },
            config,
            cancellation,
            observer,
            before_read,
            state,
        )?;
    } else {
        state.warnings.push(diagnostic(
            rule_id::SPECIAL_FILE_SKIPPED,
            "special_file_not_read",
            vec![relative_path],
        ));
    }
    Ok(None)
}

fn process_regular_file(
    file: RegularFile<'_>,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
    before_read: &mut impl FnMut(&Path),
    state: &mut ScanState,
) -> Result<(), ScanError> {
    let RegularFile {
        path,
        relative_path,
        metadata,
    } = file;
    let Some(kind) = media_kind(&relative_path) else {
        if let Some(sidecar_kind) = metadata_kind(&relative_path) {
            state.sidecars.push(DiscoveredSidecar {
                relative_path,
                kind: sidecar_kind,
            });
        }
        return Ok(());
    };
    before_read(path);
    match stream_identity(path, metadata, config.buffer_bytes, cancellation) {
        Ok((byte_len, content_sha256, changed)) => {
            state.bytes_read = state.bytes_read.saturating_add(byte_len);
            if changed {
                state.errors.push(diagnostic(
                    rule_id::SOURCE_CHANGED,
                    "source_changed_during_scan",
                    vec![relative_path.clone()],
                ));
            }
            state.media.push(DiscoveredMedia {
                relative_path,
                kind,
                byte_len,
                content_sha256,
                metadata: Vec::new(),
                live_photo: None,
                evidence: vec![RuleEvidence {
                    rule_id: rule_id::REGULAR_MEDIA.to_owned(),
                    outcome: "streamed_content_identity".to_owned(),
                }],
            });
            state.progress(ProgressStage::ContentIdentity, observer);
            Ok(())
        }
        Err(StreamError::Cancelled) => Err(ScanError::Cancelled),
        Err(StreamError::Unreadable) => {
            state.errors.push(diagnostic(
                rule_id::UNREADABLE_FILE,
                "unreadable_media",
                vec![relative_path],
            ));
            Ok(())
        }
    }
}

pub fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ScanError> {
    if cancellation.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}

fn portable_sort_key(root: &Path, path: &Path) -> Vec<u8> {
    portable_path(root, path, usize::MAX).map_or_else(
        |_| {
            path.strip_prefix(root)
                .map_or(path.as_os_str(), Path::as_os_str)
                .as_encoded_bytes()
                .to_vec()
        },
        String::into_bytes,
    )
}

fn portable_directory(root: &Path, path: &Path, max_bytes: usize) -> String {
    if path == root {
        ".".to_owned()
    } else {
        portable_path(root, path, max_bytes).unwrap_or_else(|_| "<non-unicode>".to_owned())
    }
}

#[derive(Clone, Copy)]
enum PortablePathError {
    OutsideRoot,
    NonUnicode,
    TooLong,
}

fn portable_path(root: &Path, path: &Path, max_bytes: usize) -> Result<String, PortablePathError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| PortablePathError::OutsideRoot)?;
    let mut components = Vec::new();
    for component in relative.components() {
        let value = component
            .as_os_str()
            .to_str()
            .ok_or(PortablePathError::NonUnicode)?;
        components.push(value.nfc().collect::<String>());
    }
    let portable = components.join("/");
    if portable.is_empty() {
        Err(PortablePathError::OutsideRoot)
    } else if portable.len() > max_bytes {
        Err(PortablePathError::TooLong)
    } else {
        Ok(portable)
    }
}

pub fn extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_lowercase)
}

fn media_kind(path: &str) -> Option<MediaKind> {
    match extension(path)?.as_str() {
        "jpg" | "jpeg" | "png" | "heic" | "heif" | "webp" | "gif" | "tif" | "tiff" | "dng"
        | "cr2" | "cr3" | "arw" | "raf" | "nef" => Some(MediaKind::Image),
        "mp4" | "mov" | "avi" | "mkv" | "m4v" | "3gp" => Some(MediaKind::Video),
        _ => None,
    }
}

fn metadata_kind(path: &str) -> Option<MetadataKind> {
    match extension(path)?.as_str() {
        "json" => Some(MetadataKind::Json),
        "xmp" => Some(MetadataKind::Xmp),
        _ => None,
    }
}

#[cfg(unix)]
fn lacks_read_permissions(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o444 == 0
}

#[cfg(not(unix))]
fn lacks_read_permissions(_metadata: &Metadata) -> bool {
    false
}

enum StreamError {
    Cancelled,
    Unreadable,
}

fn stream_identity(
    path: &Path,
    discovered_metadata: &Metadata,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<(u64, String, bool), StreamError> {
    if lacks_read_permissions(discovered_metadata) {
        return Err(StreamError::Unreadable);
    }
    let before = FileSnapshot::from_metadata(discovered_metadata);
    let mut file = File::open(path).map_err(|_| StreamError::Unreadable)?;
    let opened = file.metadata().map_err(|_| StreamError::Unreadable)?;
    let opened_snapshot = FileSnapshot::from_metadata(&opened);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut bytes_read = 0_u64;
    while bytes_read < opened_snapshot.len {
        if cancellation.is_cancelled() {
            return Err(StreamError::Cancelled);
        }
        let remaining = opened_snapshot.len - bytes_read;
        let read_limit =
            usize::try_from(remaining).map_or(buffer.len(), |value| value.min(buffer.len()));
        let read = file
            .read(&mut buffer[..read_limit])
            .map_err(|_| StreamError::Unreadable)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes_read = bytes_read
            .checked_add(read as u64)
            .ok_or(StreamError::Unreadable)?;
    }
    let after_open = file.metadata().map_err(|_| StreamError::Unreadable)?;
    let after_path = fs::metadata(path).map_err(|_| StreamError::Unreadable)?;
    let changed = before != opened_snapshot
        || opened_snapshot != FileSnapshot::from_metadata(&after_open)
        || opened_snapshot != FileSnapshot::from_metadata(&after_path)
        || bytes_read != opened_snapshot.len;
    Ok((bytes_read, format!("{:x}", digest.finalize()), changed))
}
