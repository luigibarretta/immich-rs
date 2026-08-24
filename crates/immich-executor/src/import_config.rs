use std::path::PathBuf;

use immich_rs_core::{Cancellation, SourceKind};
use immich_rs_sources::{
    AlbumMode, ApplePhotosScanConfig, NoProgress, ResolvedFolderPlan, ScanError, TakeoutScanConfig,
    scan_apple_photos_inputs_resolved, scan_google_takeout_inputs_resolved,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass, UploadExecutionConfig};

/// Complete scan and execution identity for one Google Takeout import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoutImportConfig {
    /// Directory or split-ZIP reconciliation and resource limits.
    pub source: TakeoutScanConfig,
    /// Upload verification, concurrency and retry limits.
    pub upload: UploadExecutionConfig,
}

impl Default for TakeoutImportConfig {
    fn default() -> Self {
        let upload = UploadExecutionConfig::default();
        Self {
            source: TakeoutScanConfig {
                scan: upload.scan.clone(),
                ..TakeoutScanConfig::default()
            },
            upload,
        }
    }
}

impl TakeoutImportConfig {
    pub(crate) fn validate(&self) -> Result<(), ExecutorError> {
        self.upload.validate()?;
        self.source
            .validate()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))?;
        if self.source.scan != self.upload.scan {
            return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
        }
        Ok(())
    }

    pub(crate) fn identity_sha256(&self) -> Result<String, ExecutorError> {
        self.validate()?;
        let identity = TakeoutConfigurationIdentity {
            upload_sha256: self.upload.identity_sha256()?,
            max_archives: self.source.max_archives,
            max_archive_entry_bytes: self.source.max_archive_entry_bytes,
            max_compression_ratio: self.source.max_compression_ratio,
            compression_ratio_grace_bytes: self.source.compression_ratio_grace_bytes,
        };
        let bytes = serde_json::to_vec(&identity)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

/// Complete scan and execution identity for one Apple Photos import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplePhotosImportConfig {
    /// Directory or split-ZIP reconciliation and resource limits.
    pub source: ApplePhotosScanConfig,
    /// Upload verification, concurrency and retry limits.
    pub upload: UploadExecutionConfig,
}

impl Default for ApplePhotosImportConfig {
    fn default() -> Self {
        let upload = UploadExecutionConfig::default();
        Self {
            source: ApplePhotosScanConfig {
                scan: upload.scan.clone(),
                ..ApplePhotosScanConfig::default()
            },
            upload,
        }
    }
}

impl ApplePhotosImportConfig {
    pub(crate) fn validate(&self) -> Result<(), ExecutorError> {
        self.upload.validate()?;
        self.source
            .validate()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))?;
        if self.source.scan != self.upload.scan {
            return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
        }
        Ok(())
    }

    pub(crate) fn identity_sha256(&self) -> Result<String, ExecutorError> {
        self.validate()?;
        let identity = AppleConfigurationIdentity {
            upload_sha256: self.upload.identity_sha256()?,
            max_archives: self.source.max_archives,
            max_archive_entry_bytes: self.source.max_archive_entry_bytes,
            max_compression_ratio: self.source.max_compression_ratio,
            compression_ratio_grace_bytes: self.source.compression_ratio_grace_bytes,
            album_mode: match self.source.album_mode {
                AlbumMode::None => "none",
                AlbumMode::Folder => "folder",
                AlbumMode::Path => "path",
            },
            album_path_joiner: &self.source.album_path_joiner,
        };
        let bytes = serde_json::to_vec(&identity)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub trait ImportConfig: Sync {
    fn validate_import(&self) -> Result<(), ExecutorError>;
    fn identity(&self) -> Result<String, ExecutorError>;
    fn source_kind(&self) -> SourceKind;
    fn upload(&self) -> &UploadExecutionConfig;
    fn archive_limits(&self) -> ArchiveLimits;

    fn scan_resolved(
        &self,
        inputs: &[PathBuf],
        source_label: &str,
        cancellation: &impl Cancellation,
    ) -> Result<ResolvedFolderPlan, ScanError>;
}

#[derive(Clone, Copy)]
pub struct ArchiveLimits {
    pub max_entry_bytes: u64,
    pub max_compression_ratio: u64,
    pub compression_ratio_grace_bytes: u64,
}

impl ImportConfig for TakeoutImportConfig {
    fn validate_import(&self) -> Result<(), ExecutorError> {
        self.validate()
    }

    fn identity(&self) -> Result<String, ExecutorError> {
        self.identity_sha256()
    }

    fn source_kind(&self) -> SourceKind {
        SourceKind::GoogleTakeout
    }

    fn upload(&self) -> &UploadExecutionConfig {
        &self.upload
    }

    fn archive_limits(&self) -> ArchiveLimits {
        ArchiveLimits {
            max_entry_bytes: self.source.max_archive_entry_bytes,
            max_compression_ratio: self.source.max_compression_ratio,
            compression_ratio_grace_bytes: self.source.compression_ratio_grace_bytes,
        }
    }

    fn scan_resolved(
        &self,
        inputs: &[PathBuf],
        source_label: &str,
        cancellation: &impl Cancellation,
    ) -> Result<ResolvedFolderPlan, ScanError> {
        scan_google_takeout_inputs_resolved(
            inputs,
            source_label,
            &self.source,
            cancellation,
            &mut NoProgress,
        )
    }
}

impl ImportConfig for ApplePhotosImportConfig {
    fn validate_import(&self) -> Result<(), ExecutorError> {
        self.validate()
    }

    fn identity(&self) -> Result<String, ExecutorError> {
        self.identity_sha256()
    }

    fn source_kind(&self) -> SourceKind {
        SourceKind::ApplePhotos
    }

    fn upload(&self) -> &UploadExecutionConfig {
        &self.upload
    }

    fn archive_limits(&self) -> ArchiveLimits {
        ArchiveLimits {
            max_entry_bytes: self.source.max_archive_entry_bytes,
            max_compression_ratio: self.source.max_compression_ratio,
            compression_ratio_grace_bytes: self.source.compression_ratio_grace_bytes,
        }
    }

    fn scan_resolved(
        &self,
        inputs: &[PathBuf],
        source_label: &str,
        cancellation: &impl Cancellation,
    ) -> Result<ResolvedFolderPlan, ScanError> {
        scan_apple_photos_inputs_resolved(
            inputs,
            source_label,
            &self.source,
            cancellation,
            &mut NoProgress,
        )
    }
}

#[derive(Serialize)]
struct TakeoutConfigurationIdentity {
    upload_sha256: String,
    max_archives: usize,
    max_archive_entry_bytes: u64,
    max_compression_ratio: u64,
    compression_ratio_grace_bytes: u64,
}

#[derive(Serialize)]
struct AppleConfigurationIdentity<'a> {
    upload_sha256: String,
    max_archives: usize,
    max_archive_entry_bytes: u64,
    max_compression_ratio: u64,
    compression_ratio_grace_bytes: u64,
    album_mode: &'a str,
    album_path_joiner: &'a str,
}
