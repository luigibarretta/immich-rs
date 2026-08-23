use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::{Cancellation, UploadOperation, UploadPlan};
use immich_rs_sources::{ResolvedFolderPlan, ResolvedSourceFile};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization as _;
use zip::{CompressionMethod, ZipArchive};

use crate::import_staging_fs::{
    cleanup_exact_root, create_private_directory, private_file, validate_real_directory,
    validate_real_file,
};
use crate::{ExecutorError, ExecutorErrorClass, TakeoutImportConfig};

const STAGING_PREFIX: &str = ".immich-rs-stage-";

pub struct ImportStaging {
    root: PathBuf,
    buffer_bytes: usize,
    max_entry_bytes: u64,
    max_compression_ratio: u64,
    compression_ratio_grace_bytes: u64,
}

pub struct PreparedImportAsset<'a> {
    pub media_path: PathBuf,
    pub file_name: String,
    pub sha1_base64: String,
    pub xmp: Option<(PathBuf, u64)>,
    staged_paths: Vec<PathBuf>,
    _staging: &'a ImportStaging,
}

impl ImportStaging {
    pub(crate) fn open(
        checkpoint: &Path,
        plan: &UploadPlan,
        config: &TakeoutImportConfig,
    ) -> Result<Self, ExecutorError> {
        config.validate()?;
        plan.validate()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
        let parent = checkpoint
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        validate_real_directory(parent)?;
        let plan_bytes = serde_json::to_vec(plan)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        let digest = format!("{:x}", Sha256::digest(plan_bytes));
        let root = parent.join(format!("{STAGING_PREFIX}{digest}"));
        cleanup_exact_root(&root)?;
        create_private_directory(&root)?;
        Ok(Self {
            root,
            buffer_bytes: config.upload.verification_buffer_bytes,
            max_entry_bytes: config.source.max_archive_entry_bytes,
            max_compression_ratio: config.source.max_compression_ratio,
            compression_ratio_grace_bytes: config.source.compression_ratio_grace_bytes,
        })
    }

    pub(crate) fn prepare<'a>(
        &'a self,
        resolved: &ResolvedFolderPlan,
        operation: &UploadOperation,
        cancellation: &impl Cancellation,
    ) -> Result<PreparedImportAsset<'a>, ExecutorError> {
        check_cancelled(cancellation)?;
        let media = resolved_source(
            resolved,
            &operation.relative_path,
            operation.byte_len,
            &operation.content_sha256,
        )?;
        let sidecar = operation
            .xmp_sidecar
            .as_ref()
            .map(|expected| {
                resolved_source(
                    resolved,
                    &expected.relative_path,
                    expected.byte_len,
                    &expected.content_sha256,
                )
            })
            .transpose()?;
        let archive_backed = media.archive_index().is_some();
        if sidecar.is_some_and(|source| source.archive_index().is_some() != archive_backed) {
            return Err(ExecutorError::new(ExecutorErrorClass::Invariant));
        }
        let staged_bytes = media
            .byte_len()
            .saturating_add(sidecar.map_or(0, ResolvedSourceFile::byte_len));
        if archive_backed && staged_bytes > self.max_entry_bytes.saturating_mul(2) {
            return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
        }
        let file_name = portable_file_name(&operation.relative_path)?;
        if !archive_backed {
            return Ok(PreparedImportAsset {
                media_path: media.native_path().to_path_buf(),
                file_name,
                sha1_base64: media.content_sha1_base64().to_owned(),
                xmp: sidecar.map(|source| (source.native_path().to_path_buf(), source.byte_len())),
                staged_paths: Vec::new(),
                _staging: self,
            });
        }
        let media_path = self.stage_entry(
            media,
            &operation.relative_path,
            &operation.operation_id,
            "media",
            cancellation,
        )?;
        let xmp = match (sidecar, operation.xmp_sidecar.as_ref()) {
            (Some(source), Some(expected)) => match self.stage_entry(
                source,
                &expected.relative_path,
                &operation.operation_id,
                "xmp",
                cancellation,
            ) {
                Ok(path) => Some((path, source.byte_len())),
                Err(error) => {
                    let _cleanup_result = fs::remove_file(&media_path);
                    return Err(error);
                }
            },
            (None, None) => None,
            _ => return Err(ExecutorError::new(ExecutorErrorClass::Invariant)),
        };
        let mut staged_paths = vec![media_path.clone()];
        if let Some((path, _)) = &xmp {
            staged_paths.push(path.clone());
        }
        Ok(PreparedImportAsset {
            media_path,
            file_name,
            sha1_base64: media.content_sha1_base64().to_owned(),
            xmp,
            staged_paths,
            _staging: self,
        })
    }

    fn stage_entry(
        &self,
        source: &ResolvedSourceFile,
        portable_path: &str,
        operation_id: &str,
        suffix: &str,
        cancellation: &impl Cancellation,
    ) -> Result<PathBuf, ExecutorError> {
        let index = source
            .archive_index()
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        let final_path = self.root.join(format!("{operation_id}.{suffix}"));
        let temporary_path = self.root.join(format!("{operation_id}.{suffix}.tmp"));
        let result = self.stage_entry_inner(
            source,
            portable_path,
            index,
            &temporary_path,
            &final_path,
            cancellation,
        );
        if result.is_err() {
            let _cleanup_result = fs::remove_file(&temporary_path);
            let _cleanup_result = fs::remove_file(&final_path);
        }
        result.map(|()| final_path)
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_entry_inner(
        &self,
        source: &ResolvedSourceFile,
        portable_path: &str,
        index: usize,
        temporary_path: &Path,
        final_path: &Path,
        cancellation: &impl Cancellation,
    ) -> Result<(), ExecutorError> {
        check_cancelled(cancellation)?;
        validate_real_file(source.native_path())?;
        let file = File::open(source.native_path()).map_err(source_changed)?;
        let before = file.metadata().map_err(source_changed)?;
        let mut archive = ZipArchive::new(file).map_err(source_changed)?;
        let mut entry = archive.by_index(index).map_err(source_changed)?;
        validate_entry(self, &entry, portable_path, source.byte_len())?;
        let mut output = private_file(temporary_path)?;
        let mut buffer = vec![0_u8; self.buffer_bytes];
        let mut sha256 = Sha256::new();
        let mut sha1 = Sha1::new();
        let mut observed = 0_u64;
        loop {
            check_cancelled(cancellation)?;
            let count = entry.read(&mut buffer).map_err(source_changed)?;
            if count == 0 {
                break;
            }
            observed = observed.saturating_add(count as u64);
            if observed > source.byte_len() {
                return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
            }
            sha256.update(&buffer[..count]);
            sha1.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(destination_error)?;
        }
        drop(entry);
        let file = archive.into_inner();
        let after = file.metadata().map_err(source_changed)?;
        if metadata_changed(&before, &after)
            || observed != source.byte_len()
            || format!("{:x}", sha256.finalize()) != source.content_sha256()
            || STANDARD.encode(sha1.finalize()) != source.content_sha1_base64()
        {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        }
        output.sync_all().map_err(destination_error)?;
        drop(output);
        fs::rename(temporary_path, final_path).map_err(destination_error)
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for ImportStaging {
    fn drop(&mut self) {
        let _cleanup_result = cleanup_exact_root(&self.root);
    }
}

impl Drop for PreparedImportAsset<'_> {
    fn drop(&mut self) {
        for path in &self.staged_paths {
            let _cleanup_result = fs::remove_file(path);
        }
    }
}

fn resolved_source<'a>(
    resolved: &'a ResolvedFolderPlan,
    portable_path: &str,
    byte_len: u64,
    content_sha256: &str,
) -> Result<&'a ResolvedSourceFile, ExecutorError> {
    let source = resolved
        .source_file(portable_path)
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    if source.byte_len() != byte_len || source.content_sha256() != content_sha256 {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(source)
}

fn validate_entry<R: Read>(
    staging: &ImportStaging,
    entry: &zip::read::ZipFile<'_, R>,
    expected_path: &str,
    expected_len: u64,
) -> Result<(), ExecutorError> {
    let valid = portable_entry_path(entry)? == expected_path
        && !entry.encrypted()
        && !entry.is_symlink()
        && entry.is_file()
        && matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        )
        && entry.size() == expected_len
        && entry.size() <= staging.max_entry_bytes
        && (entry.size() <= staging.compression_ratio_grace_bytes
            || (entry.compressed_size() > 0
                && entry.size() / entry.compressed_size() <= staging.max_compression_ratio));
    valid
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))
}

fn portable_entry_path<R: Read>(
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<String, ExecutorError> {
    if entry.name().contains(['\\', '\0'])
        || entry
            .name()
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    let enclosed = entry
        .enclosed_name()
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    let mut components = Vec::new();
    for component in enclosed.components() {
        let Component::Normal(value) = component else {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        };
        let value = value
            .to_str()
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
        components.push(value.nfc().collect::<String>());
    }
    Ok(components.join("/"))
}

fn portable_file_name(path: &str) -> Result<String, ExecutorError> {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && !value.contains(['/', '\\']))
        .map(str::to_owned)
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))
}

fn metadata_changed(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.created().ok() != after.created().ok()
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ExecutorError> {
    (!cancellation.is_cancelled())
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Cancelled))
}

fn source_changed(_error: impl std::fmt::Debug) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::SourceChanged)
}

fn destination_error(_error: impl std::fmt::Debug) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::Destination)
}
