use std::time::Duration;

use immich_rs_application::{
    AlbumMode, ApplePhotosImportConfig, FolderScanConfig, PicasaImportConfig, TakeoutImportConfig,
    UploadExecutionConfig,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::WebConfigError;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Folder,
    GoogleTakeout,
    ApplePhotos,
    Picasa,
}

impl SourceKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::GoogleTakeout => "Google Takeout",
            Self::ApplePhotos => "Apple Photos",
            Self::Picasa => "Picasa",
        }
    }
}

#[derive(Clone, Debug)]
pub enum SourceSettings {
    Folder(FolderScanConfig),
    GoogleTakeout(TakeoutImportConfig),
    ApplePhotos(ApplePhotosImportConfig),
    Picasa(PicasaImportConfig),
}

impl SourceSettings {
    pub(super) fn from_raw(
        kind: SourceKind,
        scan: RawFolderScanConfig,
        options: &RawImportOptions,
        upload: RawUploadConfig,
    ) -> Result<Self, WebConfigError> {
        let scan = scan.into_config()?;
        let upload = upload.into_config(scan.clone())?;
        match kind {
            SourceKind::Folder if options.is_empty() && upload == folder_upload(&scan) => {
                Ok(Self::Folder(scan))
            }
            SourceKind::Folder => Err(WebConfigError::new(
                "folder profiles cannot configure import options",
            )),
            SourceKind::GoogleTakeout if options.has_album_options() => Err(WebConfigError::new(
                "Google Takeout profile has unsupported album options",
            )),
            SourceKind::GoogleTakeout => {
                let mut config = TakeoutImportConfig {
                    upload,
                    ..TakeoutImportConfig::default()
                };
                options.apply_archive(
                    &mut config.source.max_archives,
                    &mut config.source.max_archive_entry_bytes,
                    &mut config.source.max_compression_ratio,
                    &mut config.source.compression_ratio_grace_bytes,
                );
                config.source.scan = scan;
                config.source.validate().map_err(scan_error)?;
                Ok(Self::GoogleTakeout(config))
            }
            SourceKind::ApplePhotos if options.has_picasa_options() => Err(WebConfigError::new(
                "Apple Photos profile has unsupported Picasa options",
            )),
            SourceKind::ApplePhotos => {
                let mut config = ApplePhotosImportConfig {
                    upload,
                    ..ApplePhotosImportConfig::default()
                };
                options.apply_archive(
                    &mut config.source.max_archives,
                    &mut config.source.max_archive_entry_bytes,
                    &mut config.source.max_compression_ratio,
                    &mut config.source.compression_ratio_grace_bytes,
                );
                options.apply_albums(
                    &mut config.source.album_mode,
                    &mut config.source.album_path_joiner,
                )?;
                config.source.scan = scan;
                config.source.validate().map_err(scan_error)?;
                Ok(Self::ApplePhotos(config))
            }
            SourceKind::Picasa => {
                let mut config = PicasaImportConfig {
                    upload,
                    ..PicasaImportConfig::default()
                };
                options.apply_archive(
                    &mut config.source.max_archives,
                    &mut config.source.max_archive_entry_bytes,
                    &mut config.source.max_compression_ratio,
                    &mut config.source.compression_ratio_grace_bytes,
                );
                options.apply_albums(
                    &mut config.source.album_mode,
                    &mut config.source.album_path_joiner,
                )?;
                config.source.picasa_albums =
                    options.picasa_albums.unwrap_or(config.source.picasa_albums);
                config.source.filename_date =
                    options.filename_date.unwrap_or(config.source.filename_date);
                config.source.scan = scan;
                config.source.validate().map_err(scan_error)?;
                Ok(Self::Picasa(config))
            }
        }
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        match self {
            Self::Folder(_) => SourceKind::Folder,
            Self::GoogleTakeout(_) => SourceKind::GoogleTakeout,
            Self::ApplePhotos(_) => SourceKind::ApplePhotos,
            Self::Picasa(_) => SourceKind::Picasa,
        }
    }

    pub(super) const fn maximum_inputs(&self) -> usize {
        match self {
            Self::Folder(_) => 1,
            Self::GoogleTakeout(config) => config.source.max_archives,
            Self::ApplePhotos(config) => config.source.max_archives,
            Self::Picasa(config) => config.source.max_archives,
        }
    }

    #[must_use]
    pub const fn scan(&self) -> &FolderScanConfig {
        match self {
            Self::Folder(config) => config,
            Self::GoogleTakeout(config) => &config.source.scan,
            Self::ApplePhotos(config) => &config.source.scan,
            Self::Picasa(config) => &config.source.scan,
        }
    }

    pub(super) fn update_digest(&self, digest: &mut Sha256) {
        digest.update([self.kind() as u8]);
        let (scan, upload) = match self {
            Self::Folder(scan) => (scan, None),
            Self::GoogleTakeout(config) => {
                update_archive(
                    digest,
                    config.source.max_archives,
                    config.source.max_archive_entry_bytes,
                    config.source.max_compression_ratio,
                    config.source.compression_ratio_grace_bytes,
                );
                (&config.source.scan, Some(&config.upload))
            }
            Self::ApplePhotos(config) => {
                update_archive(
                    digest,
                    config.source.max_archives,
                    config.source.max_archive_entry_bytes,
                    config.source.max_compression_ratio,
                    config.source.compression_ratio_grace_bytes,
                );
                update_album(
                    digest,
                    config.source.album_mode,
                    &config.source.album_path_joiner,
                );
                (&config.source.scan, Some(&config.upload))
            }
            Self::Picasa(config) => {
                update_archive(
                    digest,
                    config.source.max_archives,
                    config.source.max_archive_entry_bytes,
                    config.source.max_compression_ratio,
                    config.source.compression_ratio_grace_bytes,
                );
                update_album(
                    digest,
                    config.source.album_mode,
                    &config.source.album_path_joiner,
                );
                digest.update([
                    u8::from(config.source.picasa_albums),
                    u8::from(config.source.filename_date),
                ]);
                (&config.source.scan, Some(&config.upload))
            }
        };
        update_scan(digest, scan);
        if let Some(config) = upload {
            update_upload(digest, config);
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawFolderScanConfig {
    buffer_bytes: Option<usize>,
    max_entries: Option<usize>,
    max_directory_entries: Option<usize>,
    max_path_bytes: Option<usize>,
    case_sensitive: Option<bool>,
}

impl RawFolderScanConfig {
    fn into_config(self) -> Result<FolderScanConfig, WebConfigError> {
        let defaults = FolderScanConfig::default();
        let config = FolderScanConfig {
            buffer_bytes: self.buffer_bytes.unwrap_or(defaults.buffer_bytes),
            max_entries: self.max_entries.unwrap_or(defaults.max_entries),
            max_directory_entries: self
                .max_directory_entries
                .unwrap_or(defaults.max_directory_entries),
            max_path_bytes: self.max_path_bytes.unwrap_or(defaults.max_path_bytes),
            case_sensitive: self.case_sensitive.unwrap_or(defaults.case_sensitive),
        };
        let valid = (4_096..=4 * 1_024 * 1_024).contains(&config.buffer_bytes)
            && config.max_entries > 0
            && config.max_directory_entries > 0
            && (64..=65_536).contains(&config.max_path_bytes);
        valid
            .then_some(config)
            .ok_or_else(|| WebConfigError::new("source scan limits are invalid"))
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawImportOptions {
    max_archives: Option<usize>,
    max_archive_entry_bytes: Option<u64>,
    max_compression_ratio: Option<u64>,
    compression_ratio_grace_bytes: Option<u64>,
    album_mode: Option<String>,
    album_path_joiner: Option<String>,
    picasa_albums: Option<bool>,
    filename_date: Option<bool>,
}

impl RawImportOptions {
    const fn is_empty(&self) -> bool {
        !self.has_archive_options() && !self.has_album_options()
    }

    const fn has_archive_options(&self) -> bool {
        self.max_archives.is_some()
            || self.max_archive_entry_bytes.is_some()
            || self.max_compression_ratio.is_some()
            || self.compression_ratio_grace_bytes.is_some()
    }

    const fn has_album_options(&self) -> bool {
        self.album_mode.is_some() || self.album_path_joiner.is_some() || self.has_picasa_options()
    }

    const fn has_picasa_options(&self) -> bool {
        self.picasa_albums.is_some() || self.filename_date.is_some()
    }

    fn apply_archive(&self, count: &mut usize, bytes: &mut u64, ratio: &mut u64, grace: &mut u64) {
        *count = self.max_archives.unwrap_or(*count);
        *bytes = self.max_archive_entry_bytes.unwrap_or(*bytes);
        *ratio = self.max_compression_ratio.unwrap_or(*ratio);
        *grace = self.compression_ratio_grace_bytes.unwrap_or(*grace);
    }

    fn apply_albums(
        &self,
        mode: &mut AlbumMode,
        joiner: &mut String,
    ) -> Result<(), WebConfigError> {
        if let Some(value) = &self.album_mode {
            *mode = match value.as_str() {
                "none" => AlbumMode::None,
                "folder" => AlbumMode::Folder,
                "path" => AlbumMode::Path,
                _ => return Err(WebConfigError::new("source album mode is invalid")),
            };
        }
        if let Some(value) = &self.album_path_joiner {
            joiner.clone_from(value);
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawUploadConfig {
    verification_buffer_bytes: Option<usize>,
    concurrency: Option<usize>,
    max_attempts_per_operation: Option<u32>,
    max_retries_per_run: Option<u32>,
    retry_base_delay_ms: Option<u64>,
    retry_delay_cap_ms: Option<u64>,
}

impl RawUploadConfig {
    fn into_config(self, scan: FolderScanConfig) -> Result<UploadExecutionConfig, WebConfigError> {
        let defaults = UploadExecutionConfig::default();
        let config = UploadExecutionConfig {
            scan,
            verification_buffer_bytes: self
                .verification_buffer_bytes
                .unwrap_or(defaults.verification_buffer_bytes),
            concurrency: self.concurrency.unwrap_or(defaults.concurrency),
            max_attempts_per_operation: self
                .max_attempts_per_operation
                .unwrap_or(defaults.max_attempts_per_operation),
            max_retries_per_run: self
                .max_retries_per_run
                .unwrap_or(defaults.max_retries_per_run),
            retry_base_delay: Duration::from_millis(self.retry_base_delay_ms.unwrap_or(100)),
            retry_delay_cap: Duration::from_millis(self.retry_delay_cap_ms.unwrap_or(10_000)),
        };
        let valid = (4_096..=4 * 1_024 * 1_024).contains(&config.verification_buffer_bytes)
            && (1..=8).contains(&config.concurrency)
            && (1..=10).contains(&config.max_attempts_per_operation)
            && config.max_retries_per_run <= 10_000
            && !config.retry_base_delay.is_zero()
            && config.retry_base_delay <= config.retry_delay_cap
            && config.retry_delay_cap <= Duration::from_secs(300);
        valid
            .then_some(config)
            .ok_or_else(|| WebConfigError::new("source upload limits are invalid"))
    }
}

fn folder_upload(scan: &FolderScanConfig) -> UploadExecutionConfig {
    UploadExecutionConfig {
        scan: scan.clone(),
        ..UploadExecutionConfig::default()
    }
}

fn update_scan(digest: &mut Sha256, config: &FolderScanConfig) {
    digest.update(config.buffer_bytes.to_le_bytes());
    digest.update(config.max_entries.to_le_bytes());
    digest.update(config.max_directory_entries.to_le_bytes());
    digest.update(config.max_path_bytes.to_le_bytes());
    digest.update([u8::from(config.case_sensitive)]);
}

fn update_archive(digest: &mut Sha256, count: usize, bytes: u64, ratio: u64, grace: u64) {
    digest.update(count.to_le_bytes());
    digest.update(bytes.to_le_bytes());
    digest.update(ratio.to_le_bytes());
    digest.update(grace.to_le_bytes());
}

fn update_album(digest: &mut Sha256, mode: AlbumMode, joiner: &str) {
    let value = match mode {
        AlbumMode::None => 0,
        AlbumMode::Folder => 1,
        AlbumMode::Path => 2,
    };
    digest.update([value]);
    digest.update(joiner.len().to_le_bytes());
    digest.update(joiner.as_bytes());
}

fn update_upload(digest: &mut Sha256, config: &UploadExecutionConfig) {
    digest.update(config.verification_buffer_bytes.to_le_bytes());
    digest.update(config.concurrency.to_le_bytes());
    digest.update(config.max_attempts_per_operation.to_le_bytes());
    digest.update(config.max_retries_per_run.to_le_bytes());
    digest.update(config.retry_base_delay.as_millis().to_le_bytes());
    digest.update(config.retry_delay_cap.as_millis().to_le_bytes());
}

const fn scan_error(_: immich_rs_application::ScanError) -> WebConfigError {
    WebConfigError::new("source scan limits are invalid")
}
