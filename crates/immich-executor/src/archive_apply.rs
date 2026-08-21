use std::path::{Component, Path, PathBuf};

use immich_rs_client::{ArchiveDownload, ImmichReadClient, NegotiatedServer};
use immich_rs_core::{
    ARCHIVE_APPLY_REPORT_SCHEMA_VERSION, ArchiveApplyReport, ArchiveAsset, ArchiveManifest,
    Cancellation, CancellationToken,
};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{ExecutorError, ExecutorErrorClass};

const VERIFY_BUFFER_BYTES: usize = 64 * 1024;

/// Materialize immutable originals below one safe local destination.
pub async fn apply_archive(
    manifest: &ArchiveManifest,
    destination: &Path,
    client: &ImmichReadClient,
    negotiated: &NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<ArchiveApplyReport, ExecutorError> {
    manifest
        .validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    if negotiated.compatibility() != &manifest.server {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    ensure_destination(destination).await?;
    let manifest_bytes = serde_json::to_vec(manifest)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(manifest_bytes));
    let mut report = ArchiveApplyReport {
        schema_version: ARCHIVE_APPLY_REPORT_SCHEMA_VERSION,
        manifest_sha256,
        downloaded: 0,
        already_complete: 0,
        bytes_written: 0,
        retries: 0,
    };
    for asset in &manifest.assets {
        if cancellation.is_cancelled() {
            return Err(ExecutorError::new(ExecutorErrorClass::Cancelled));
        }
        let final_path = destination.join(&asset.target_path);
        if path_exists(&final_path).await? {
            verify_existing(&final_path, asset, cancellation).await?;
            report.already_complete = report.already_complete.saturating_add(1);
            continue;
        }
        ensure_asset_directory(destination, &asset.asset_id).await?;
        let part_path = part_path(&final_path)?;
        remove_stale_part(&part_path).await?;
        let result = download_asset(client, negotiated, asset, &part_path, cancellation).await;
        if let Err(error) = result {
            remove_created_part(&part_path).await;
            return Err(error);
        }
        if fs::rename(&part_path, &final_path).await.is_err() {
            remove_created_part(&part_path).await;
            return Err(ExecutorError::new(ExecutorErrorClass::Destination));
        }
        report.downloaded = report.downloaded.saturating_add(1);
        report.bytes_written = report.bytes_written.saturating_add(asset.byte_len);
    }
    report
        .validate(manifest.summary.assets)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    Ok(report)
}

async fn download_asset(
    client: &ImmichReadClient,
    negotiated: &NegotiatedServer,
    asset: &ArchiveAsset,
    part_path: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutorError> {
    let mut download = client
        .download_original(negotiated, &asset.asset_id, cancellation)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Client))?;
    validate_content_length(&download, asset.byte_len)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(part_path)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    let mut hasher = Sha1::new();
    let mut byte_len = 0_u64;
    while let Some(chunk) = download
        .next_chunk(cancellation)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Client))?
    {
        byte_len = byte_len
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Client))?;
        if byte_len > asset.byte_len {
            return Err(ExecutorError::new(ExecutorErrorClass::Client));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    }
    file.sync_all()
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    if byte_len != asset.byte_len || format!("{:x}", hasher.finalize()) != asset.checksum_sha1 {
        return Err(ExecutorError::new(ExecutorErrorClass::Client));
    }
    Ok(())
}

fn validate_content_length(download: &ArchiveDownload, expected: u64) -> Result<(), ExecutorError> {
    if download
        .content_length()
        .is_some_and(|length| length != expected)
    {
        Err(ExecutorError::new(ExecutorErrorClass::Client))
    } else {
        Ok(())
    }
}

async fn verify_existing(
    path: &Path,
    asset: &ArchiveAsset,
    cancellation: &impl Cancellation,
) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    if !metadata.file_type().is_file() || metadata.len() != asset.byte_len {
        return Err(ExecutorError::new(ExecutorErrorClass::Destination));
    }
    let mut file = File::open(path)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    let mut buffer = vec![0_u8; VERIFY_BUFFER_BYTES];
    let mut hasher = Sha1::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(ExecutorError::new(ExecutorErrorClass::Cancelled));
        }
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != asset.checksum_sha1 {
        return Err(ExecutorError::new(ExecutorErrorClass::Destination));
    }
    Ok(())
}

async fn ensure_destination(destination: &Path) -> Result<(), ExecutorError> {
    if destination.as_os_str().is_empty() {
        return Err(ExecutorError::new(ExecutorErrorClass::Destination));
    }
    let mut current = PathBuf::new();
    for component in destination.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(component.as_os_str());
            }
            Component::CurDir | Component::ParentDir => {
                return Err(ExecutorError::new(ExecutorErrorClass::Destination));
            }
        }
        create_or_validate_directory(&current).await?;
    }
    ensure_real_directory(destination).await
}

async fn ensure_asset_directory(
    destination: &Path,
    asset_id: &str,
) -> Result<PathBuf, ExecutorError> {
    let assets = destination.join("assets");
    create_or_validate_directory(&assets).await?;
    let asset = assets.join(asset_id);
    create_or_validate_directory(&asset).await?;
    Ok(asset)
}

async fn create_or_validate_directory(path: &Path) -> Result<(), ExecutorError> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)
            .await
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination)),
        Ok(_) | Err(_) => Err(ExecutorError::new(ExecutorErrorClass::Destination)),
    }
}

async fn ensure_real_directory(path: &Path) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(ExecutorError::new(ExecutorErrorClass::Destination))
    }
}

async fn path_exists(path: &Path) -> Result<bool, ExecutorError> {
    match fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ExecutorError::new(ExecutorErrorClass::Destination)),
    }
}

fn part_path(final_path: &Path) -> Result<PathBuf, ExecutorError> {
    let name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Destination))?;
    Ok(final_path.with_file_name(format!(".{name}.immich-rs.part")))
}

async fn remove_stale_part(path: &Path) -> Result<(), ExecutorError> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(path)
            .await
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(ExecutorError::new(ExecutorErrorClass::Destination)),
    }
}

async fn remove_created_part(path: &Path) {
    let _cleanup_result = fs::remove_file(path).await;
}
