use immich_rs_sources::TakeoutScanConfig;
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

#[derive(Serialize)]
struct TakeoutConfigurationIdentity {
    upload_sha256: String,
    max_archives: usize,
    max_archive_entry_bytes: u64,
    max_compression_ratio: u64,
    compression_ratio_grace_bytes: u64,
}
