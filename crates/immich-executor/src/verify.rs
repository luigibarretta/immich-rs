use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::{Cancellation, UploadPlan};
use immich_rs_sources::{NoProgress, ResolvedFolderPlan, scan_folder_resolved};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass, UploadExecutionConfig};

pub struct VerifiedAsset {
    pub media_path: PathBuf,
    pub file_name: String,
    pub sha1_base64: String,
    pub xmp: Option<(PathBuf, u64)>,
}

pub fn verify_source(
    plan: &UploadPlan,
    root: &Path,
    config: &UploadExecutionConfig,
    cancellation: &impl Cancellation,
) -> Result<BTreeMap<String, VerifiedAsset>, ExecutorError> {
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    if plan.configuration_sha256 != config.identity_sha256()? {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let mut progress = NoProgress;
    let resolved = scan_folder_resolved(
        root,
        &plan.source.label,
        &config.scan,
        cancellation,
        &mut progress,
    )
    .map_err(|error| match error {
        immich_rs_sources::ScanError::Cancelled => {
            ExecutorError::new(ExecutorErrorClass::Cancelled)
        }
        _ => ExecutorError::new(ExecutorErrorClass::SourceChanged),
    })?;
    require_same_normalized_plan(plan, &resolved)?;
    let mut verified = BTreeMap::new();
    for operation in &plan.operations {
        check_cancelled(cancellation)?;
        let media = resolved
            .source_file(&operation.relative_path)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
        let identity = stream_identity(
            media.native_path(),
            operation.byte_len,
            operation.created_at_unix_ms,
            operation.modified_at_unix_ms,
            config.verification_buffer_bytes,
            cancellation,
        )?;
        if identity.sha256 != operation.content_sha256 {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        }
        let xmp = verify_sidecar(&resolved, operation, config, cancellation)?;
        let file_name = operation
            .relative_path
            .rsplit_once('/')
            .map_or(operation.relative_path.as_str(), |(_, name)| name)
            .to_owned();
        verified.insert(
            operation.operation_id.clone(),
            VerifiedAsset {
                media_path: media.native_path().to_path_buf(),
                file_name,
                sha1_base64: identity.sha1_base64,
                xmp,
            },
        );
    }
    Ok(verified)
}

fn require_same_normalized_plan(
    plan: &UploadPlan,
    resolved: &ResolvedFolderPlan,
) -> Result<(), ExecutorError> {
    let bytes = serde_json::to_vec(&resolved.plan)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != plan.normalized_plan_sha256 || resolved.plan.source != plan.source {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(())
}

fn verify_sidecar(
    resolved: &ResolvedFolderPlan,
    operation: &immich_rs_core::UploadOperation,
    config: &UploadExecutionConfig,
    cancellation: &impl Cancellation,
) -> Result<Option<(PathBuf, u64)>, ExecutorError> {
    let Some(expected) = &operation.xmp_sidecar else {
        return Ok(None);
    };
    let source = resolved
        .source_file(&expected.relative_path)
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    let identity = stream_content(
        source.native_path(),
        expected.byte_len,
        config.verification_buffer_bytes,
        cancellation,
    )?;
    if identity.sha256 != expected.content_sha256 {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(Some((
        source.native_path().to_path_buf(),
        expected.byte_len,
    )))
}

struct StreamIdentity {
    sha256: String,
    sha1_base64: String,
}

fn stream_identity(
    path: &Path,
    expected_len: u64,
    expected_created: i64,
    expected_modified: i64,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<StreamIdentity, ExecutorError> {
    let before = source_timestamps(path, expected_len)?;
    if before != (expected_created, expected_modified) {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    let identity = stream_content(path, expected_len, buffer_bytes, cancellation)?;
    let after = source_timestamps(path, expected_len)?;
    if after != before {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(identity)
}

fn stream_content(
    path: &Path,
    expected_len: u64,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<StreamIdentity, ExecutorError> {
    let file =
        File::open(path).map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    let mut reader = BufReader::with_capacity(buffer_bytes, file);
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut bytes_read = 0_u64;
    loop {
        check_cancelled(cancellation)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
        if count == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(count as u64)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
        if bytes_read > expected_len {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        }
        sha256.update(&buffer[..count]);
        sha1.update(&buffer[..count]);
    }
    if bytes_read != expected_len {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(StreamIdentity {
        sha256: format!("{:x}", sha256.finalize()),
        sha1_base64: STANDARD.encode(sha1.finalize()),
    })
}

pub fn source_timestamps(path: &Path, expected_len: u64) -> Result<(i64, i64), ExecutorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != expected_len
    {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    let modified = metadata
        .modified()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    let created = metadata.created().map_or(modified, std::convert::identity);
    Ok((unix_millis(created)?, unix_millis(modified)?))
}

fn unix_millis(value: SystemTime) -> Result<i64, ExecutorError> {
    let duration = value
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::SourceChanged))
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ExecutorError> {
    if cancellation.is_cancelled() {
        Err(ExecutorError::new(ExecutorErrorClass::Cancelled))
    } else {
        Ok(())
    }
}
