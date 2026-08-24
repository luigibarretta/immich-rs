use crate::{
    MIGRATION_PLAN_SCHEMA_VERSION, MediaKind, MigrationAlbum, MigrationAsset, MigrationPlan,
    MigrationPlanSummary, MigrationPlanValidationError, MigrationServer, NormalizedMetadata,
    ServerCompatibility, ServerVersion, UploadRole,
};

fn server(seed: char) -> MigrationServer {
    MigrationServer {
        compatibility: ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: seed.to_string().repeat(64),
        },
        origin_sha256: seed
            .to_ascii_uppercase()
            .to_ascii_lowercase()
            .to_string()
            .repeat(64),
    }
}

fn plan() -> MigrationPlan {
    let asset = MigrationAsset {
        source_asset_id: "00000000-0000-4000-8000-000000000001".to_owned(),
        operation_id: "c".repeat(64),
        original_file_name: "synthetic.jpg".to_owned(),
        media_kind: MediaKind::Image,
        byte_len: 42,
        checksum_sha1: "d".repeat(40),
        content_sha256: "e".repeat(64),
        created_at_unix_ms: 1_704_067_200_000,
        modified_at_unix_ms: 1_704_067_201_000,
        normalized_metadata: Some(NormalizedMetadata {
            description: Some("synthetic description".to_owned()),
            taken_at_utc: Some("2024-01-01T00:00:00Z".to_owned()),
            location: None,
            albums: Vec::new(),
        }),
        role: UploadRole::Standalone,
    };
    MigrationPlan {
        schema_version: MIGRATION_PLAN_SCHEMA_VERSION,
        source_server: server('a'),
        destination_server: server('b'),
        source_fingerprint_sha256: "f".repeat(64),
        configuration_sha256: "1".repeat(64),
        assets: vec![asset],
        albums: vec![MigrationAlbum {
            name: "Synthetic Album".to_owned(),
            member_operation_ids: vec!["c".repeat(64)],
        }],
        summary: MigrationPlanSummary {
            assets: 1,
            media_bytes: 42,
            metadata_updates: 1,
            album_creates: 1,
            album_memberships: 1,
            max_mutations: 4,
        },
    }
}

#[test]
fn migration_plan_is_strict_and_two_server_bound() {
    let valid = plan();
    assert_eq!(valid.validate(), Ok(()));

    let mut same_origin = valid.clone();
    same_origin.destination_server.origin_sha256 = same_origin.source_server.origin_sha256.clone();
    assert_eq!(
        same_origin.validate(),
        Err(MigrationPlanValidationError::InvalidIdentity)
    );

    let mut changed_member = valid.clone();
    changed_member.albums[0].member_operation_ids[0] = "9".repeat(64);
    assert_eq!(
        changed_member.validate(),
        Err(MigrationPlanValidationError::InvalidAlbum)
    );

    let mut changed_summary = valid;
    changed_summary.summary.max_mutations = 3;
    assert_eq!(
        changed_summary.validate(),
        Err(MigrationPlanValidationError::SummaryMismatch)
    );
}

#[test]
fn migration_plan_rejects_album_metadata_inside_assets() {
    let mut invalid = plan();
    if let Some(metadata) = &mut invalid.assets[0].normalized_metadata {
        metadata.albums.push("Synthetic Album".to_owned());
    }
    assert_eq!(
        invalid.validate(),
        Err(MigrationPlanValidationError::InvalidAsset)
    );
}
