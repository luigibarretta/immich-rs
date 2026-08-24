use super::{
    ARCHIVE_APPLY_REPORT_SCHEMA_VERSION, ARCHIVE_MANIFEST_SCHEMA_VERSION, ArchiveApplyReport,
    ArchiveAsset, ArchiveManifest, ArchiveManifestSummary, Cancellation, CancellationToken,
    CandidateAsset, GeoCoordinates, MediaKind, NORMALIZED_PLAN_SCHEMA_VERSION,
    NORMALIZED_PLAN_SCHEMA_VERSION_V2, NORMALIZED_PLAN_SCHEMA_VERSION_V3,
    NORMALIZED_PLAN_SCHEMA_VERSION_V4, NeverCancel, NormalizedMetadata, NormalizedPlan,
    PlanSummary, ServerCompatibility, ServerVersion, SourceDescriptor, SourceKind,
    UPLOAD_PLAN_SCHEMA_VERSION, UPLOAD_PLAN_SCHEMA_VERSION_V2, UnicodeNormalization,
    UploadOperation, UploadPlan, UploadPlanSummary, UploadRole,
};

fn valid_archive_manifest() -> ArchiveManifest {
    ArchiveManifest {
        schema_version: ARCHIVE_MANIFEST_SCHEMA_VERSION,
        server: ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "a".repeat(64),
        },
        configuration_sha256: "b".repeat(64),
        assets: vec![ArchiveAsset {
            asset_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            original_file_name: "synthetic-image.jpg".to_owned(),
            target_path: "assets/00000000-0000-4000-8000-000000000001/synthetic-image.jpg"
                .to_owned(),
            media_kind: MediaKind::Image,
            byte_len: 4,
            checksum_sha1: "c".repeat(40),
        }],
        summary: ArchiveManifestSummary {
            assets: 1,
            media_bytes: 4,
        },
    }
}

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
fn normalized_plan_v3_is_apple_photos_only() {
    let mut plan = valid_plan();
    plan.schema_version = NORMALIZED_PLAN_SCHEMA_VERSION_V3;
    assert!(plan.validate().is_err());
    plan.source.kind = SourceKind::ApplePhotos;
    plan.assets[0].normalized_metadata = Some(NormalizedMetadata {
        albums: vec!["Synthetic album".to_owned()],
        ..NormalizedMetadata::default()
    });
    assert!(plan.validate().is_ok());
    plan.schema_version = NORMALIZED_PLAN_SCHEMA_VERSION_V2;
    assert!(plan.validate().is_err());
}

#[test]
fn normalized_plan_v4_is_picasa_only() {
    let mut plan = valid_plan();
    plan.schema_version = NORMALIZED_PLAN_SCHEMA_VERSION_V4;
    assert!(plan.validate().is_err());
    plan.source.kind = SourceKind::Picasa;
    plan.assets[0].normalized_metadata = Some(NormalizedMetadata {
        albums: vec!["Albums / Synthetic".to_owned()],
        ..NormalizedMetadata::default()
    });
    assert!(plan.validate().is_ok());
    plan.source.kind = SourceKind::ApplePhotos;
    assert!(plan.validate().is_err());
}

#[test]
fn normalized_metadata_rejects_noncanonical_values() {
    let mut plan = valid_plan();
    plan.schema_version = NORMALIZED_PLAN_SCHEMA_VERSION_V2;
    plan.source.kind = SourceKind::GoogleTakeout;
    let metadata = NormalizedMetadata {
        description: Some("synthetic description".to_owned()),
        taken_at_utc: Some("2024-01-01T00:00:00Z".to_owned()),
        location: Some(GeoCoordinates {
            latitude: "01.0".to_owned(),
            longitude: "2".to_owned(),
        }),
        albums: vec!["Synthetic album".to_owned()],
    };
    plan.assets[0].normalized_metadata = Some(metadata);
    assert!(plan.validate().is_err());
    if let Some(metadata) = &mut plan.assets[0].normalized_metadata {
        metadata.location = None;
        metadata.taken_at_utc = Some("2024-99-99T00:00:00Z".to_owned());
    }
    assert!(plan.validate().is_err());
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
            normalized_metadata: None,
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
            normalized_metadata: None,
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
            metadata_updates: 0,
            album_creates: 0,
            album_memberships: 0,
            max_mutations: 0,
        },
        operations,
    };
    plan.validate()?;
    let encoded = serde_json::to_vec(&plan)?;
    let decoded: UploadPlan = serde_json::from_slice(&encoded)?;
    assert_eq!(decoded, plan);
    Ok(())
}

#[test]
fn import_plan_summary_is_schema_bound_and_counts_mutation_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let metadata = NormalizedMetadata {
        description: Some("synthetic description".to_owned()),
        taken_at_utc: Some("2024-01-01T00:00:00Z".to_owned()),
        location: None,
        albums: vec!["Synthetic album".to_owned()],
    };
    let operation = UploadOperation {
        operation_id: "b".repeat(64),
        relative_path: "image.jpg".to_owned(),
        media_kind: MediaKind::Image,
        byte_len: 4,
        content_sha256: "c".repeat(64),
        created_at_unix_ms: 1_704_067_200_000,
        modified_at_unix_ms: 1_704_067_200_000,
        xmp_sidecar: None,
        normalized_metadata: Some(metadata),
        role: UploadRole::Standalone,
    };
    let operations = vec![operation];
    let mut plan = UploadPlan {
        schema_version: UPLOAD_PLAN_SCHEMA_VERSION_V2,
        normalized_plan_sha256: "d".repeat(64),
        source: SourceDescriptor {
            kind: SourceKind::GoogleTakeout,
            label: "synthetic-takeout".to_owned(),
            fingerprint_sha256: "e".repeat(64),
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::Nfc,
        },
        configuration_sha256: "f".repeat(64),
        server: ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "a".repeat(64),
        },
        summary: UploadPlanSummary::from_operations(UPLOAD_PLAN_SCHEMA_VERSION_V2, &operations),
        operations,
    };
    assert_eq!(plan.summary.metadata_updates, 1);
    assert_eq!(plan.summary.album_creates, 1);
    assert_eq!(plan.summary.album_memberships, 1);
    assert_eq!(plan.summary.max_mutations, 4);
    plan.validate()?;
    plan.source.kind = SourceKind::Folder;
    assert!(plan.validate().is_err());
    Ok(())
}

#[test]
fn archive_manifest_and_report_are_stable_and_strict() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = valid_archive_manifest();
    manifest.validate()?;
    let encoded = serde_json::to_vec(&manifest)?;
    let decoded: ArchiveManifest = serde_json::from_slice(&encoded)?;
    assert_eq!(decoded, manifest);
    let report = ArchiveApplyReport {
        schema_version: ARCHIVE_APPLY_REPORT_SCHEMA_VERSION,
        manifest_sha256: "d".repeat(64),
        downloaded: 1,
        already_complete: 0,
        bytes_written: 4,
        retries: 0,
    };
    report.validate(1)?;
    Ok(())
}

#[test]
fn archive_manifest_rejects_unsafe_names_duplicates_and_ordering() {
    let mut manifest = valid_archive_manifest();
    manifest.assets[0].original_file_name = "../escape.jpg".to_owned();
    assert!(manifest.validate().is_err());

    manifest = valid_archive_manifest();
    let mut duplicate = manifest.assets[0].clone();
    duplicate.target_path.push('z');
    manifest.assets.push(duplicate);
    manifest.summary.assets = 2;
    manifest.summary.media_bytes = 8;
    assert!(manifest.validate().is_err());

    manifest = valid_archive_manifest();
    manifest.assets[0].original_file_name = "CON.jpg".to_owned();
    manifest.assets[0].target_path =
        "assets/00000000-0000-4000-8000-000000000001/CON.jpg".to_owned();
    assert!(manifest.validate().is_err());
}
