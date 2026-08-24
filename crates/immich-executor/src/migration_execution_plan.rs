use std::collections::BTreeMap;

use immich_rs_client::migration_plan_sha256;
use immich_rs_core::{
    MigrationPlan, NormalizedMetadata, SourceDescriptor, SourceKind, UPLOAD_PLAN_SCHEMA_VERSION_V2,
    UnicodeNormalization, UploadOperation, UploadPlan, UploadPlanSummary,
};

use crate::{ExecutorError, ExecutorErrorClass};

pub fn execution_plan(plan: &MigrationPlan) -> Result<UploadPlan, ExecutorError> {
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    let migration_sha256 = migration_plan_sha256(plan)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    let mut albums = BTreeMap::<String, Vec<String>>::new();
    for album in &plan.albums {
        for operation_id in &album.member_operation_ids {
            albums
                .entry(operation_id.clone())
                .or_default()
                .push(album.name.clone());
        }
    }
    let operations = plan
        .assets
        .iter()
        .map(|asset| {
            let mut metadata = asset.normalized_metadata.clone().unwrap_or_default();
            metadata.albums = albums.remove(&asset.operation_id).unwrap_or_default();
            let normalized_metadata =
                (metadata != NormalizedMetadata::default()).then_some(metadata);
            UploadOperation {
                operation_id: asset.operation_id.clone(),
                relative_path: format!(
                    "assets/{}/{}",
                    asset.source_asset_id, asset.original_file_name
                ),
                media_kind: asset.media_kind,
                byte_len: asset.byte_len,
                content_sha256: asset.content_sha256.clone(),
                created_at_unix_ms: asset.created_at_unix_ms,
                modified_at_unix_ms: asset.modified_at_unix_ms,
                xmp_sidecar: None,
                normalized_metadata,
                role: asset.role.clone(),
            }
        })
        .collect::<Vec<_>>();
    if !albums.is_empty() {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let summary = UploadPlanSummary::from_operations(UPLOAD_PLAN_SCHEMA_VERSION_V2, &operations);
    if summary.operations != plan.summary.assets
        || summary.media_bytes != plan.summary.media_bytes
        || summary.metadata_updates != plan.summary.metadata_updates
        || summary.album_creates != plan.summary.album_creates
        || summary.album_memberships != plan.summary.album_memberships
        || summary.max_mutations != plan.summary.max_mutations
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let execution = UploadPlan {
        schema_version: UPLOAD_PLAN_SCHEMA_VERSION_V2,
        normalized_plan_sha256: migration_sha256,
        source: SourceDescriptor {
            kind: SourceKind::Immich,
            label: "immich-migration".to_owned(),
            fingerprint_sha256: plan.source_fingerprint_sha256.clone(),
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::Nfc,
        },
        configuration_sha256: plan.configuration_sha256.clone(),
        server: plan.destination_server.compatibility.clone(),
        operations,
        summary,
    };
    execution
        .validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    Ok(execution)
}
