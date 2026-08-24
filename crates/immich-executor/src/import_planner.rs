use std::collections::BTreeMap;

use immich_rs_core::{
    LivePhotoRole, MetadataKind, ServerCompatibility, UPLOAD_PLAN_SCHEMA_VERSION_V2,
    UploadOperation, UploadPlan, UploadPlanSummary, UploadRole, UploadSidecar,
};
use immich_rs_sources::ResolvedFolderPlan;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::import_config::ImportConfig;
use crate::{
    ApplePhotosImportConfig, ExecutorError, ExecutorErrorClass, PicasaImportConfig,
    TakeoutImportConfig,
};

const FALLBACK_TIMESTAMP_UNIX_MS: i64 = 0;

/// Convert one resolved Takeout scan into a server-bound immutable import plan.
pub fn create_takeout_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &TakeoutImportConfig,
) -> Result<UploadPlan, ExecutorError> {
    create_import_upload_plan(resolved, server, config)
}

/// Convert one resolved Apple Photos scan into a server-bound immutable import plan.
pub fn create_apple_photos_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &ApplePhotosImportConfig,
) -> Result<UploadPlan, ExecutorError> {
    create_import_upload_plan(resolved, server, config)
}

/// Convert one resolved Picasa scan into a server-bound immutable import plan.
pub fn create_picasa_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &PicasaImportConfig,
) -> Result<UploadPlan, ExecutorError> {
    create_import_upload_plan(resolved, server, config)
}

pub fn create_import_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &impl ImportConfig,
) -> Result<UploadPlan, ExecutorError> {
    config.validate_import()?;
    resolved
        .plan
        .validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    if !resolved.plan.errors.is_empty()
        || resolved.plan.source.kind != config.source_kind()
        || server.version.major != 3
        || server.version.minor != 1
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let videos = live_video_operations(resolved);
    let mut operations = Vec::with_capacity(resolved.plan.assets.len());
    for asset in &resolved.plan.assets {
        let source = source_for(
            resolved,
            &asset.relative_path,
            asset.byte_len,
            &asset.content_sha256,
        )?;
        let timestamps = import_timestamps(asset, source, config.source_kind())?;
        operations.push(UploadOperation {
            operation_id: asset.operation_id.clone(),
            relative_path: asset.relative_path.clone(),
            media_kind: asset.media_kind,
            byte_len: asset.byte_len,
            content_sha256: asset.content_sha256.clone(),
            created_at_unix_ms: timestamps.0,
            modified_at_unix_ms: timestamps.1,
            xmp_sidecar: xmp_sidecar(resolved, asset)?,
            normalized_metadata: asset.normalized_metadata.clone(),
            role: upload_role(asset, &videos)?,
        });
    }
    let plan = UploadPlan {
        schema_version: UPLOAD_PLAN_SCHEMA_VERSION_V2,
        normalized_plan_sha256: compact_sha256(&resolved.plan)?,
        source: resolved.plan.source.clone(),
        configuration_sha256: config.identity()?,
        server,
        summary: UploadPlanSummary::from_operations(UPLOAD_PLAN_SCHEMA_VERSION_V2, &operations),
        operations,
    };
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    Ok(plan)
}

fn source_for<'a>(
    resolved: &'a ResolvedFolderPlan,
    path: &str,
    byte_len: u64,
    content_sha256: &str,
) -> Result<&'a immich_rs_sources::ResolvedSourceFile, ExecutorError> {
    let source = resolved
        .source_file(path)
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    if source.byte_len() != byte_len || source.content_sha256() != content_sha256 {
        return Err(ExecutorError::new(ExecutorErrorClass::Invariant));
    }
    Ok(source)
}

fn import_timestamps(
    asset: &immich_rs_core::CandidateAsset,
    source: &immich_rs_sources::ResolvedSourceFile,
    source_kind: immich_rs_core::SourceKind,
) -> Result<(i64, i64), ExecutorError> {
    if let Some(value) = asset
        .normalized_metadata
        .as_ref()
        .and_then(|metadata| metadata.taken_at_utc.as_deref())
    {
        let instant = OffsetDateTime::parse(value, &Rfc3339)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
        let millis = i64::try_from(instant.unix_timestamp_nanos() / 1_000_000)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
        return Ok((millis, millis));
    }
    if matches!(
        source_kind,
        immich_rs_core::SourceKind::ApplePhotos | immich_rs_core::SourceKind::Picasa
    ) {
        let created = source
            .created_at_unix_ms()
            .or_else(|| source.modified_at_unix_ms())
            .unwrap_or(FALLBACK_TIMESTAMP_UNIX_MS);
        let modified = source.modified_at_unix_ms().unwrap_or(created);
        return Ok((created, modified));
    }
    Ok((FALLBACK_TIMESTAMP_UNIX_MS, FALLBACK_TIMESTAMP_UNIX_MS))
}

fn xmp_sidecar(
    resolved: &ResolvedFolderPlan,
    asset: &immich_rs_core::CandidateAsset,
) -> Result<Option<UploadSidecar>, ExecutorError> {
    let xmp = asset
        .metadata
        .iter()
        .filter(|metadata| metadata.kind == MetadataKind::Xmp)
        .collect::<Vec<_>>();
    let [metadata] = xmp.as_slice() else {
        return if xmp.is_empty() {
            Ok(None)
        } else {
            Err(ExecutorError::new(ExecutorErrorClass::UnsupportedMetadata))
        };
    };
    let source = resolved
        .source_file(&metadata.relative_path)
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    Ok(Some(UploadSidecar {
        relative_path: metadata.relative_path.clone(),
        byte_len: source.byte_len(),
        content_sha256: source.content_sha256().to_owned(),
    }))
}

fn live_video_operations(resolved: &ResolvedFolderPlan) -> BTreeMap<&str, &str> {
    resolved
        .plan
        .assets
        .iter()
        .filter_map(|asset| {
            asset.live_photo.as_ref().and_then(|member| {
                (member.role == LivePhotoRole::Video)
                    .then_some((member.pair_id.as_str(), asset.operation_id.as_str()))
            })
        })
        .collect()
}

fn upload_role(
    asset: &immich_rs_core::CandidateAsset,
    videos: &BTreeMap<&str, &str>,
) -> Result<UploadRole, ExecutorError> {
    let Some(member) = &asset.live_photo else {
        return Ok(UploadRole::Standalone);
    };
    match member.role {
        LivePhotoRole::Video => Ok(UploadRole::LivePhotoVideo {
            pair_id: member.pair_id.clone(),
        }),
        LivePhotoRole::Image => Ok(UploadRole::LivePhotoImage {
            pair_id: member.pair_id.clone(),
            video_operation_id: (*videos
                .get(member.pair_id.as_str())
                .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?)
            .to_owned(),
        }),
    }
}

fn compact_sha256(value: &impl serde::Serialize) -> Result<String, ExecutorError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
