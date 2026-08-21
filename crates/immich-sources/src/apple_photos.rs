use std::path::PathBuf;

use immich_rs_core::{
    Cancellation, NORMALIZED_PLAN_SCHEMA_VERSION_V3, NormalizedMetadata, NormalizedPlan,
    RuleEvidence, SourceKind, rule_id,
};
use sha2::{Digest, Sha256};

use crate::{FolderScanConfig, ProgressObserver, ScanError, ScanStrategy, scan_resolved_internal};

/// Explicit folder-to-album mapping for an Apple Photos export.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AlbumMode {
    /// Do not infer album membership from folders.
    #[default]
    None,
    /// Use only the immediate parent folder name.
    Folder,
    /// Join every parent folder component with the configured separator.
    Path,
}

/// Bounded options for Apple Photos directory or independent ZIP inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplePhotosScanConfig {
    /// Common streaming and filesystem limits.
    pub scan: FolderScanConfig,
    /// Maximum number of independent ZIP downloads.
    pub max_archives: usize,
    /// Maximum declared uncompressed bytes for one ZIP entry.
    pub max_archive_entry_bytes: u64,
    /// Maximum compression expansion ratio beyond the grace threshold.
    pub max_compression_ratio: u64,
    /// Entry size at or below which the ratio check is omitted.
    pub compression_ratio_grace_bytes: u64,
    /// Explicit folder-derived album behavior.
    pub album_mode: AlbumMode,
    /// Bounded separator used only by `AlbumMode::Path`.
    pub album_path_joiner: String,
}

impl Default for ApplePhotosScanConfig {
    fn default() -> Self {
        Self {
            scan: FolderScanConfig::default(),
            max_archives: 64,
            max_archive_entry_bytes: 1_099_511_627_776,
            max_compression_ratio: 200,
            compression_ratio_grace_bytes: 1_048_576,
            album_mode: AlbumMode::None,
            album_path_joiner: " - ".to_owned(),
        }
    }
}

impl ApplePhotosScanConfig {
    pub(crate) fn validate(&self) -> Result<(), ScanError> {
        self.scan.validate()?;
        if !(1..=64).contains(&self.max_archives) {
            return Err(ScanError::InvalidConfiguration(
                "max_archives must be in 1..=64",
            ));
        }
        if self.max_archive_entry_bytes == 0 || !(1..=10_000).contains(&self.max_compression_ratio)
        {
            return Err(ScanError::InvalidConfiguration(
                "archive byte and compression limits must be positive and bounded",
            ));
        }
        let joiner = &self.album_path_joiner;
        if joiner.is_empty()
            || joiner.len() > 32
            || joiner.contains(['/', '\\'])
            || joiner.chars().any(char::is_control)
        {
            return Err(ScanError::InvalidConfiguration(
                "album_path_joiner must be 1..=32 safe UTF-8 bytes",
            ));
        }
        Ok(())
    }
}

/// Scan one Apple Photos folder or a bounded set of independent iCloud ZIPs.
pub fn scan_apple_photos_inputs(
    inputs: &[PathBuf],
    source_label: &str,
    config: &ApplePhotosScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    config.validate()?;
    if let [input] = inputs {
        if std::fs::symlink_metadata(input).is_ok_and(|metadata| {
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
        }) {
            let resolved = scan_resolved_internal(
                input,
                source_label,
                &config.scan,
                cancellation,
                observer,
                &mut |_| {},
                ScanStrategy {
                    source_kind: SourceKind::ApplePhotos,
                    schema_version: NORMALIZED_PLAN_SCHEMA_VERSION_V3,
                    reconcile_state: crate::reconcile::reconcile,
                    skip_path: export_noise_path,
                },
            )?;
            return finish_plan(resolved.plan, config);
        }
    }
    if inputs.iter().any(|input| {
        std::fs::symlink_metadata(input).is_ok_and(|metadata| metadata.file_type().is_dir())
    }) {
        return Err(ScanError::UnsupportedLayout(
            "directory and ZIP inputs cannot be mixed",
        ));
    }
    crate::apple_archive::scan_archives(inputs, source_label, config, cancellation, observer)
}

pub fn finish_plan(
    mut plan: NormalizedPlan,
    config: &ApplePhotosScanConfig,
) -> Result<NormalizedPlan, ScanError> {
    for asset in &mut plan.assets {
        let Some(album) = album_for(&asset.relative_path, config) else {
            continue;
        };
        let metadata = asset
            .normalized_metadata
            .get_or_insert_with(NormalizedMetadata::default);
        metadata.albums.push(album);
        metadata.albums.sort();
        metadata.albums.dedup();
        asset.evidence.push(RuleEvidence {
            rule_id: rule_id::APPLE_FOLDER_ALBUM.to_owned(),
            outcome: "folder_album_selected".to_owned(),
        });
        asset.evidence.sort();
        asset.evidence.dedup();
    }
    refresh_fingerprint(&mut plan);
    plan.validate()?;
    Ok(plan)
}

pub fn export_noise_path(path: &str) -> Option<String> {
    let components = path.split('/').collect::<Vec<_>>();
    components
        .iter()
        .position(|component| {
            component.eq_ignore_ascii_case("@eaDir")
                || component.eq_ignore_ascii_case(".Spotlight-V100")
                || component.eq_ignore_ascii_case(".photostructure")
                || component.eq_ignore_ascii_case("Recently Deleted")
                || component.eq_ignore_ascii_case(".DS_Store")
                || component.eq_ignore_ascii_case("Thumbs.db")
                || component.starts_with("._")
        })
        .map(|index| components[..=index].join("/"))
}

fn album_for(path: &str, config: &ApplePhotosScanConfig) -> Option<String> {
    let parent = path.rsplit_once('/')?.0;
    match config.album_mode {
        AlbumMode::None => None,
        AlbumMode::Folder => parent.rsplit('/').next().map(str::to_owned),
        AlbumMode::Path => Some(
            parent
                .split('/')
                .collect::<Vec<_>>()
                .join(&config.album_path_joiner),
        ),
    }
}

fn refresh_fingerprint(plan: &mut NormalizedPlan) {
    let mut digest = Sha256::new();
    digest.update(b"apple-photos-source-v1\0");
    for asset in &plan.assets {
        update(&mut digest, &asset.relative_path);
        update(&mut digest, &asset.content_sha256);
        for sidecar in &asset.metadata {
            update(&mut digest, &sidecar.relative_path);
            update(&mut digest, &sidecar.rule_id);
        }
        if let Some(metadata) = &asset.normalized_metadata {
            for album in &metadata.albums {
                update(&mut digest, album);
            }
        }
    }
    for diagnostic in plan.warnings.iter().chain(&plan.errors) {
        update(&mut digest, &diagnostic.rule_id);
        for path in &diagnostic.paths {
            update(&mut digest, path);
        }
    }
    plan.source.fingerprint_sha256 = format!("{:x}", digest.finalize());
}

fn update(digest: &mut Sha256, field: &str) {
    digest.update(field.len().to_le_bytes());
    digest.update(field.as_bytes());
}
