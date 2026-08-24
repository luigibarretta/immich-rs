use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_client::{ClientError, ClientErrorClass, ImmichReadClient, NegotiatedServer};
use immich_rs_core::{CancellationToken, MigrationAsset, MigrationPlan};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;

use crate::import_staging_fs::{
    cleanup_exact_root, create_private_directory, private_file, validate_real_directory,
    validate_real_file,
};
use crate::verify::VerifiedAsset;
use crate::{ExecutorError, ExecutorErrorClass};

const STAGING_PREFIX: &str = ".immich-rs-migration-stage-";

pub struct MigrationStaging {
    root: PathBuf,
    max_asset_bytes: u64,
}

pub struct PreparedMigrationAsset<'a> {
    path: PathBuf,
    file_name: String,
    sha1_base64: String,
    _staging: &'a MigrationStaging,
}

impl MigrationStaging {
    pub fn open(
        checkpoint: &Path,
        plan: &MigrationPlan,
        max_asset_bytes: u64,
    ) -> Result<Self, ExecutorError> {
        plan.validate()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
        if max_asset_bytes == 0 {
            return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
        }
        let parent = checkpoint
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        validate_real_directory(parent)?;
        let bytes = serde_json::to_vec(plan)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        let root = parent.join(format!("{STAGING_PREFIX}{digest}"));
        cleanup_exact_root(&root)?;
        create_private_directory(&root)?;
        Ok(Self {
            root,
            max_asset_bytes,
        })
    }

    pub async fn prepare<'a>(
        &'a self,
        client: &ImmichReadClient,
        server: &NegotiatedServer,
        asset: &MigrationAsset,
        cancellation: &CancellationToken,
    ) -> Result<PreparedMigrationAsset<'a>, ExecutorError> {
        if asset.byte_len > self.max_asset_bytes {
            return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
        }
        let final_path = self.root.join(format!("{}.media", asset.operation_id));
        let temporary_path = self.root.join(format!("{}.media.tmp", asset.operation_id));
        let result = self
            .download(
                client,
                server,
                asset,
                &temporary_path,
                &final_path,
                cancellation,
            )
            .await;
        if result.is_err() {
            let _cleanup_result = fs::remove_file(&temporary_path);
            let _cleanup_result = fs::remove_file(&final_path);
        }
        result?;
        Ok(PreparedMigrationAsset {
            path: final_path,
            file_name: asset.original_file_name.clone(),
            sha1_base64: hex_sha1_base64(&asset.checksum_sha1)?,
            _staging: self,
        })
    }

    async fn download(
        &self,
        client: &ImmichReadClient,
        server: &NegotiatedServer,
        asset: &MigrationAsset,
        temporary_path: &Path,
        final_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutorError> {
        let mut download = client
            .download_original(server, &asset.source_asset_id, cancellation)
            .await
            .map_err(map_client_error)?;
        if download
            .content_length()
            .is_some_and(|length| length != asset.byte_len || length > self.max_asset_bytes)
        {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        }
        let mut output = private_file(temporary_path)?;
        let mut sha1 = Sha1::new();
        let mut sha256 = Sha256::new();
        let mut observed = 0_u64;
        while let Some(chunk) = download
            .next_chunk(cancellation)
            .await
            .map_err(map_client_error)?
        {
            observed = observed
                .checked_add(chunk.len() as u64)
                .ok_or_else(source_changed)?;
            if observed > asset.byte_len || observed > self.max_asset_bytes {
                return Err(source_changed());
            }
            sha1.update(&chunk);
            sha256.update(&chunk);
            output
                .write_all(&chunk)
                .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
        }
        if observed != asset.byte_len
            || format!("{:x}", sha1.finalize()) != asset.checksum_sha1
            || format!("{:x}", sha256.finalize()) != asset.content_sha256
        {
            return Err(source_changed());
        }
        output
            .sync_all()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
        drop(output);
        fs::rename(temporary_path, final_path)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
        validate_real_file(final_path)
    }
}

impl PreparedMigrationAsset<'_> {
    pub fn verified(&self) -> VerifiedAsset {
        VerifiedAsset {
            media_path: self.path.clone(),
            file_name: self.file_name.clone(),
            sha1_base64: self.sha1_base64.clone(),
            xmp: None,
        }
    }
}

impl Drop for PreparedMigrationAsset<'_> {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_file(&self.path);
    }
}

impl Drop for MigrationStaging {
    fn drop(&mut self) {
        let _cleanup_result = cleanup_exact_root(&self.root);
    }
}

fn hex_sha1_base64(value: &str) -> Result<String, ExecutorError> {
    if value.len() != 40 {
        return Err(source_changed());
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| source_changed())?;
            u8::from_str_radix(text, 16).map_err(|_| source_changed())
        })
        .collect::<Result<Vec<_>, ExecutorError>>()?;
    Ok(STANDARD.encode(bytes))
}

const fn map_client_error(error: ClientError) -> ExecutorError {
    match error.class() {
        ClientErrorClass::Cancelled => ExecutorError::new(ExecutorErrorClass::Cancelled),
        _ => ExecutorError::new(ExecutorErrorClass::Client),
    }
}

const fn source_changed() -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::SourceChanged)
}
