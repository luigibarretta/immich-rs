use super::{
    Cancellation, CancellationToken, CandidateAsset, GeoCoordinates, MediaKind,
    NORMALIZED_PLAN_SCHEMA_VERSION, NORMALIZED_PLAN_SCHEMA_VERSION_V2, NeverCancel,
    NormalizedMetadata, NormalizedPlan, PlanSummary, ServerCompatibility, ServerVersion,
    SourceDescriptor, SourceKind, UPLOAD_PLAN_SCHEMA_VERSION, UnicodeNormalization,
    UploadOperation, UploadPlan, UploadPlanSummary, UploadRole,
};

fn valid_plan() -> NormalizedPlan {
    NormalizedPlan {
        schema_version: NORMALIZED_PLAN_SCHEMA_VERSION,
        source: SourceDescriptor {
            kind: SourceKind::Folder,
            label: "synthetic-basic".to_owned(),
            fingerprint_sha256: "a".repeat(64),
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::Nfc,
        },
        assets: vec![CandidateAsset {
            operation_id: "b".repeat(64),
            relative_path: "image.jpg".to_owned(),
            media_kind: MediaKind::Image,
            byte_len: 4,
            content_sha256: "c".repeat(64),
            metadata: Vec::new(),
            normalized_metadata: None,
            live_photo: None,
            evidence: Vec::new(),
        }],
        warnings: Vec::new(),
        errors: Vec::new(),
        summary: PlanSummary {
            assets: 1,
            sidecars: 0,
            bytes_read: 4,
        },
    }
}

#[test]
fn normalized_plan_round_trips_without_losing_schema_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = valid_plan();
    let encoded = serde_json::to_string(&plan)?;
    let decoded: NormalizedPlan = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, plan);
    decoded.validate()?;
    Ok(())
}

#[test]
fn normalized_plan_rejects_unsorted_assets() {
    let mut plan = valid_plan();
    let mut second = plan.assets[0].clone();
    second.relative_path = "another.jpg".to_owned();
    second.operation_id = "d".repeat(64);
    plan.assets.push(second);
    plan.summary.assets = 2;
    assert!(plan.validate().is_err());
}

#[test]
fn normalized_plan_v2_is_takeout_only_and_v1_rejects_resolved_metadata() {
    let mut plan = valid_plan();
    plan.assets[0].normalized_metadata = Some(NormalizedMetadata {
        description: Some("synthetic description".to_owned()),
        taken_at_utc: Some("2024-01-01T00:00:00Z".to_owned()),
        location: Some(GeoCoordinates {
            latitude: "0".to_owned(),
            longitude: "1.5".to_owned(),
        }),
        albums: vec!["Synthetic album".to_owned()],
    });
    assert!(plan.validate().is_err());

    plan.schema_version = NORMALIZED_PLAN_SCHEMA_VERSION_V2;
    assert!(plan.validate().is_err());
    plan.source.kind = SourceKind::GoogleTakeout;
    assert!(plan.validate().is_ok());
}

#[test]
fn cancellation_is_explicit_and_clone_safe() {
    let token = CancellationToken::default();
    let observer = token.clone();
    assert!(!observer.is_cancelled());
    token.cancel();
    assert!(observer.is_cancelled());
    assert!(!NeverCancel.is_cancelled());
}

#[test]
fn upload_plan_validates_live_photo_dependencies() -> Result<(), Box<dyn std::error::Error>> {
    let normalized = valid_plan();
    let video_id = "d".repeat(64);
    let image_id = "e".repeat(64);
    let operations = vec![
        UploadOperation {
            operation_id: video_id.clone(),
            relative_path: "pair.mov".to_owned(),
            media_kind: MediaKind::Video,
            byte_len: 8,
            content_sha256: "f".repeat(64),
            created_at_unix_ms: 0,
            modified_at_unix_ms: 0,
            xmp_sidecar: None,
            role: UploadRole::LivePhotoVideo {
                pair_id: "pair-v1".to_owned(),
            },
        },
        UploadOperation {
            operation_id: image_id,
            relative_path: "pair.png".to_owned(),
            media_kind: MediaKind::Image,
            byte_len: 8,
            content_sha256: "a".repeat(64),
            created_at_unix_ms: 0,
            modified_at_unix_ms: 0,
            xmp_sidecar: None,
            role: UploadRole::LivePhotoImage {
                pair_id: "pair-v1".to_owned(),
                video_operation_id: video_id,
            },
        },
    ];
    let plan = UploadPlan {
        schema_version: UPLOAD_PLAN_SCHEMA_VERSION,
        normalized_plan_sha256: "b".repeat(64),
        source: normalized.source,
        configuration_sha256: "c".repeat(64),
        server: ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "d".repeat(64),
        },
        summary: UploadPlanSummary {
            operations: 2,
            media_bytes: 16,
            xmp_sidecars: 0,
            live_photo_pairs: 1,
        },
        operations,
    };
    plan.validate()?;
    let encoded = serde_json::to_vec(&plan)?;
    let decoded: UploadPlan = serde_json::from_slice(&encoded)?;
    assert_eq!(decoded, plan);
    Ok(())
}
