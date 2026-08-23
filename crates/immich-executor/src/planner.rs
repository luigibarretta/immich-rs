use std::collections::BTreeMap;

use immich_rs_core::{
    LivePhotoRole, MetadataKind, ServerCompatibility, UPLOAD_PLAN_SCHEMA_VERSION, UploadOperation,
    UploadPlan, UploadPlanSummary, UploadRole, UploadSidecar,
};
use immich_rs_sources::ResolvedFolderPlan;
use sha2::{Digest, Sha256};

use crate::verify::source_timestamps;
use crate::{ExecutorError, ExecutorErrorClass, UploadExecutionConfig};

/// Convert one resolved read-only scan into a server-bound immutable upload plan.
pub fn create_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &UploadExecutionConfig,
) -> Result<UploadPlan, ExecutorError> {
    config.validate()?;
    resolved
        .plan
        .validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    if !resolved.plan.errors.is_empty() {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceDiagnostics));
    }
    if server.version.major != 3 || server.version.minor != 1 {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let video_operations = live_video_operations(resolved);
    let mut operations = Vec::with_capacity(resolved.plan.assets.len());
    for asset in &resolved.plan.assets {
        let source = resolved
            .source_file(&asset.relative_path)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        if source.byte_len() != asset.byte_len || source.content_sha256() != asset.content_sha256 {
            return Err(ExecutorError::new(ExecutorErrorClass::Invariant));
        }
        let (created_at_unix_ms, modified_at_unix_ms) =
            source_timestamps(source.native_path(), asset.byte_len)?;
        let xmp_sidecar = plan_sidecar(resolved, asset)?;
        operations.push(UploadOperation {
            operation_id: asset.operation_id.clone(),
            relative_path: asset.relative_path.clone(),
            media_kind: asset.media_kind,
            byte_len: asset.byte_len,
            content_sha256: asset.content_sha256.clone(),
            created_at_unix_ms,
            modified_at_unix_ms,
            xmp_sidecar,
            normalized_metadata: None,
            role: upload_role(asset, &video_operations)?,
        });
    }
    let summary = summarize(&operations);
    let normalized_plan_sha256 = compact_sha256(&resolved.plan)?;
    let plan = UploadPlan {
        schema_version: UPLOAD_PLAN_SCHEMA_VERSION,
        normalized_plan_sha256,
        source: resolved.plan.source.clone(),
        configuration_sha256: config.identity_sha256()?,
        server,
        operations,
        summary,
    };
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    Ok(plan)
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
        LivePhotoRole::Image => {
            let video_operation_id = videos
                .get(member.pair_id.as_str())
                .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
            Ok(UploadRole::LivePhotoImage {
                pair_id: member.pair_id.clone(),
                video_operation_id: (*video_operation_id).to_owned(),
            })
        }
    }
}

fn plan_sidecar(
    resolved: &ResolvedFolderPlan,
    asset: &immich_rs_core::CandidateAsset,
) -> Result<Option<UploadSidecar>, ExecutorError> {
    if asset.metadata.len() > 1
        || asset
            .metadata
            .iter()
            .any(|metadata| metadata.kind == MetadataKind::Json)
    {
        return Err(ExecutorError::new(ExecutorErrorClass::UnsupportedMetadata));
    }
    let Some(metadata) = asset.metadata.first() else {
        return Ok(None);
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

fn compact_sha256(value: &impl serde::Serialize) -> Result<String, ExecutorError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn summarize(operations: &[UploadOperation]) -> UploadPlanSummary {
    UploadPlanSummary {
        operations: operations.len() as u64,
        media_bytes: operations.iter().map(|operation| operation.byte_len).sum(),
        xmp_sidecars: operations
            .iter()
            .filter(|operation| operation.xmp_sidecar.is_some())
            .count() as u64,
        live_photo_pairs: operations
            .iter()
            .filter(|operation| matches!(operation.role, UploadRole::LivePhotoImage { .. }))
            .count() as u64,
        metadata_updates: 0,
        album_creates: 0,
        album_memberships: 0,
        max_mutations: 0,
    }
}
