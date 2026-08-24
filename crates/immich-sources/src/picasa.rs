use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use immich_rs_core::{
    Cancellation, NORMALIZED_PLAN_SCHEMA_VERSION_V4, NormalizedMetadata, NormalizedPlan,
    PlanDiagnostic, RuleEvidence, SourceKind, rule_id,
};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, PrimitiveDateTime, Time};
use zip::ZipArchive;

use crate::apple_photos::{AlbumMode, ApplePhotosScanConfig};
use crate::archive_support::{portable_entry_path, validate_entry};
use crate::picasa_ini::PicasaDocument;
use crate::{
    FolderScanConfig, ProgressObserver, ResolvedFolderPlan, ScanError, ScanStrategy,
    scan_resolved_internal,
};

const MAX_INI_BYTES: u64 = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PicasaScanConfig {
    pub scan: FolderScanConfig,
    pub max_archives: usize,
    pub max_archive_entry_bytes: u64,
    pub max_compression_ratio: u64,
    pub compression_ratio_grace_bytes: u64,
    pub album_mode: AlbumMode,
    pub album_path_joiner: String,
    pub picasa_albums: bool,
    pub filename_date: bool,
}

impl Default for PicasaScanConfig {
    fn default() -> Self {
        let common = ApplePhotosScanConfig::default();
        Self {
            scan: common.scan,
            max_archives: common.max_archives,
            max_archive_entry_bytes: common.max_archive_entry_bytes,
            max_compression_ratio: common.max_compression_ratio,
            compression_ratio_grace_bytes: common.compression_ratio_grace_bytes,
            album_mode: AlbumMode::None,
            album_path_joiner: " / ".to_owned(),
            picasa_albums: true,
            filename_date: true,
        }
    }
}

impl PicasaScanConfig {
    pub fn validate(&self) -> Result<(), ScanError> {
        self.scan.validate()?;
        if !(1..=64).contains(&self.max_archives)
            || self.max_archive_entry_bytes == 0
            || !(1..=10_000).contains(&self.max_compression_ratio)
        {
            return Err(ScanError::InvalidConfiguration(
                "Picasa archive limits must be positive and bounded",
            ));
        }
        if self.album_path_joiner.is_empty()
            || self.album_path_joiner.len() > 32
            || self.album_path_joiner.contains('\\')
            || self.album_path_joiner.chars().any(char::is_control)
        {
            return Err(ScanError::InvalidConfiguration(
                "album_path_joiner must be 1..=32 safe UTF-8 bytes",
            ));
        }
        Ok(())
    }

    fn as_archive_config(&self) -> ApplePhotosScanConfig {
        ApplePhotosScanConfig {
            scan: self.scan.clone(),
            max_archives: self.max_archives,
            max_archive_entry_bytes: self.max_archive_entry_bytes,
            max_compression_ratio: self.max_compression_ratio,
            compression_ratio_grace_bytes: self.compression_ratio_grace_bytes,
            album_mode: AlbumMode::None,
            album_path_joiner: "-".to_owned(),
        }
    }
}

pub fn scan_picasa_inputs(
    inputs: &[PathBuf],
    source_label: &str,
    config: &PicasaScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    scan_picasa_inputs_resolved(inputs, source_label, config, cancellation, observer)
        .map(|resolved| resolved.plan)
}

pub fn scan_picasa_inputs_resolved(
    inputs: &[PathBuf],
    source_label: &str,
    config: &PicasaScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    config.validate()?;
    if let [root] = inputs
        && std::fs::symlink_metadata(root).is_ok_and(|metadata| {
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
        })
    {
        let resolved = scan_resolved_internal(
            root,
            source_label,
            &config.scan,
            cancellation,
            observer,
            &mut |_| {},
            ScanStrategy {
                source_kind: SourceKind::Picasa,
                schema_version: NORMALIZED_PLAN_SCHEMA_VERSION_V4,
                reconcile_state: crate::reconcile::reconcile,
                skip_path: |_| None,
            },
        )?;
        let documents = directory_documents(root, &resolved.plan)?;
        return finish(resolved, config, documents);
    }
    if inputs.iter().any(|input| {
        std::fs::symlink_metadata(input).is_ok_and(|metadata| metadata.file_type().is_dir())
    }) {
        return Err(ScanError::UnsupportedLayout(
            "directory and ZIP inputs cannot be mixed",
        ));
    }
    let archive_config = config.as_archive_config();
    let resolved = crate::apple_archive::scan_picasa_archives_resolved(
        inputs,
        source_label,
        &archive_config,
        cancellation,
        observer,
    )?;
    let documents = archive_documents(inputs, config, cancellation)?;
    finish(resolved, config, documents)
}

fn directory_documents(root: &Path, plan: &NormalizedPlan) -> Result<Documents, ScanError> {
    let parents = plan
        .assets
        .iter()
        .map(|asset| parent(&asset.relative_path).to_owned())
        .collect::<BTreeSet<_>>();
    let mut documents = Documents::default();
    for parent in parents {
        let path = root.join(&parent).join(".picasa.ini");
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            documents.invalid(parent);
            continue;
        }
        let file = File::open(path).map_err(|_| ScanError::InvalidRoot)?;
        documents.insert(parent, &read_bounded(file)?);
    }
    Ok(documents)
}

fn archive_documents(
    inputs: &[PathBuf],
    config: &PicasaScanConfig,
    cancellation: &impl Cancellation,
) -> Result<Documents, ScanError> {
    let mut documents = Documents::default();
    for input in inputs {
        let file =
            File::open(input).map_err(|_| ScanError::InvalidArchive("cannot open archive"))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|_| ScanError::InvalidArchive("invalid ZIP directory"))?;
        for index in 0..archive.len() {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let mut entry = archive
                .by_index(index)
                .map_err(|_| ScanError::InvalidArchive("cannot open ZIP entry"))?;
            let path = portable_entry_path(&entry, config.scan.max_path_bytes)?;
            if !path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(".picasa.ini"))
            {
                continue;
            }
            validate_entry(
                &entry,
                MAX_INI_BYTES,
                config.max_compression_ratio,
                config.compression_ratio_grace_bytes,
            )?;
            let directory = parent(&path).to_owned();
            documents.insert(directory, &read_bounded(&mut entry)?);
        }
    }
    Ok(documents)
}

fn read_bounded(mut reader: impl Read) -> Result<Vec<u8>, ScanError> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_INI_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ScanError::InvalidArchive("cannot read Picasa INI"))?;
    if bytes.len() as u64 > MAX_INI_BYTES {
        return Err(ScanError::LimitExceeded("max_picasa_ini_bytes"));
    }
    Ok(bytes)
}

#[derive(Default)]
struct Documents {
    parsed: BTreeMap<String, PicasaDocument>,
    errors: Vec<PlanDiagnostic>,
}

impl Documents {
    fn insert(&mut self, directory: String, bytes: &[u8]) {
        match crate::picasa_ini::parse(bytes) {
            Ok(document) if !self.parsed.contains_key(&directory) => {
                self.parsed.insert(directory, document);
            }
            _ => self.invalid(directory),
        }
    }

    fn invalid(&mut self, directory: String) {
        self.errors.push(crate::reconcile::diagnostic(
            rule_id::PICASA_INI_INVALID,
            "invalid_picasa_ini",
            vec![if directory.is_empty() {
                ".".to_owned()
            } else {
                directory
            }],
        ));
    }
}

fn finish(
    mut resolved: ResolvedFolderPlan,
    config: &PicasaScanConfig,
    documents: Documents,
) -> Result<ResolvedFolderPlan, ScanError> {
    for asset in &mut resolved.plan.assets {
        apply_metadata(
            asset,
            config,
            documents.parsed.get(parent(&asset.relative_path)),
        );
    }
    resolved.plan.errors.extend(documents.errors);
    resolved.plan.errors.sort();
    resolved.plan.errors.dedup();
    refresh_fingerprint(&mut resolved.plan)?;
    resolved.plan.validate()?;
    Ok(resolved)
}

fn apply_metadata(
    asset: &mut immich_rs_core::CandidateAsset,
    config: &PicasaScanConfig,
    document: Option<&PicasaDocument>,
) {
    let mut metadata = asset
        .normalized_metadata
        .take()
        .map_or_else(NormalizedMetadata::default, |value| value);
    if let Some(caption) = document.and_then(|doc| doc.captions.get(filename(&asset.relative_path)))
    {
        metadata.description = Some(caption.clone());
        evidence(asset, rule_id::PICASA_CAPTION, "picasa_caption_selected");
    }
    if config.picasa_albums
        && let Some(album) = document.and_then(|doc| doc.album.as_ref())
    {
        metadata.albums.push(album.clone());
        evidence(asset, rule_id::PICASA_ALBUM, "picasa_album_selected");
    }
    if let Some(album) = folder_album(&asset.relative_path, config) {
        metadata.albums.push(album);
        evidence(asset, rule_id::PICASA_FOLDER_ALBUM, "folder_album_selected");
    }
    if config.filename_date && metadata.taken_at_utc.is_none() {
        metadata.taken_at_utc = filename_timestamp(filename(&asset.relative_path));
        if metadata.taken_at_utc.is_some() {
            evidence(
                asset,
                rule_id::PICASA_FILENAME_DATE,
                "filename_date_selected",
            );
        }
    }
    metadata.albums.sort();
    metadata.albums.dedup();
    if metadata != NormalizedMetadata::default() {
        asset.normalized_metadata = Some(metadata);
    }
}

fn evidence(asset: &mut immich_rs_core::CandidateAsset, rule: &str, outcome: &str) {
    asset.evidence.push(RuleEvidence {
        rule_id: rule.to_owned(),
        outcome: outcome.to_owned(),
    });
    asset.evidence.sort();
    asset.evidence.dedup();
}

fn folder_album(path: &str, config: &PicasaScanConfig) -> Option<String> {
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

fn filename_timestamp(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    if bytes.len() < 15 || bytes.get(8) != Some(&b'-') {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| {
        std::str::from_utf8(bytes.get(range)?)
            .ok()?
            .parse::<u8>()
            .ok()
    };
    let year = std::str::from_utf8(bytes.get(0..4)?)
        .ok()?
        .parse::<i32>()
        .ok()?;
    let month = Month::try_from(digits(4..6)?).ok()?;
    let date = Date::from_calendar_date(year, month, digits(6..8)?).ok()?;
    let time = Time::from_hms(digits(9..11)?, digits(11..13)?, digits(13..15)?).ok()?;
    PrimitiveDateTime::new(date, time)
        .assume_utc()
        .format(&Rfc3339)
        .ok()
}

fn refresh_fingerprint(plan: &mut NormalizedPlan) -> Result<(), ScanError> {
    let mut digest = Sha256::new();
    digest.update(b"picasa-source-v1\0");
    let bytes = serde_json::to_vec(&(&plan.assets, &plan.warnings, &plan.errors))
        .map_err(|_| ScanError::InvalidConfiguration("Picasa identity serialization failed"))?;
    digest.update(bytes);
    plan.source.fingerprint_sha256 = format!("{:x}", digest.finalize());
    Ok(())
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |value| value.0)
}

fn filename(path: &str) -> &str {
    path.rsplit('/').next().map_or(path, |value| value)
}
