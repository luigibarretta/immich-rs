use std::collections::BTreeMap;

use immich_rs_client::{
    MigrationListConfig, RemoteMigrationAsset, RemoteMigrationInventory, RemoteOwnedAlbum,
};
use immich_rs_core::{
    GeoCoordinates, MediaKind, MigrationServer, NormalizedMetadata, ServerCompatibility,
    ServerVersion, UploadRole,
};

use crate::migration_plan::{MigrationPlanningConfig, assemble_migration_plan};

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
        origin_sha256: seed.to_string().repeat(64),
    }
}

fn asset(id: &str, kind: MediaKind, live: Option<&str>) -> RemoteMigrationAsset {
    RemoteMigrationAsset {
        asset_id: id.to_owned(),
        original_file_name: match kind {
            MediaKind::Image => "synthetic.jpg",
            MediaKind::Video => "synthetic.mov",
        }
        .to_owned(),
        media_kind: kind,
        byte_len: 42,
        checksum_sha1: "c".repeat(40),
        created_at_unix_ms: 1_704_067_200_000,
        modified_at_unix_ms: 1_704_067_201_000,
        normalized_metadata: Some(NormalizedMetadata {
            description: Some("synthetic".to_owned()),
            taken_at_utc: Some("2024-01-01T00:00:00Z".to_owned()),
            location: Some(GeoCoordinates {
                latitude: "1".to_owned(),
                longitude: "2".to_owned(),
            }),
            albums: Vec::new(),
        }),
        live_photo_video_id: live.map(str::to_owned),
    }
}

#[test]
fn migration_plan_binds_bytes_live_photo_and_owned_album() -> Result<(), Box<dyn std::error::Error>>
{
    let image_id = "00000000-0000-4000-8000-000000000001";
    let video_id = "00000000-0000-4000-8000-000000000002";
    let inventory = RemoteMigrationInventory {
        assets: vec![
            asset(image_id, MediaKind::Image, Some(video_id)),
            asset(video_id, MediaKind::Video, None),
        ],
        albums: vec![RemoteOwnedAlbum {
            album_id: "00000000-0000-4000-8000-000000000003".to_owned(),
            name: "Synthetic Album".to_owned(),
            asset_ids: vec![image_id.to_owned()],
        }],
    };
    let hashes = [
        (image_id.to_owned(), "d".repeat(64)),
        (video_id.to_owned(), "e".repeat(64)),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    let plan = assemble_migration_plan(
        inventory,
        server('a'),
        server('b'),
        &hashes,
        MigrationPlanningConfig::default(),
    )?;
    assert_eq!(plan.summary.assets, 2);
    assert_eq!(plan.summary.max_mutations, 6);
    assert_eq!(plan.albums.len(), 1);
    assert!(matches!(
        plan.assets[0].role,
        UploadRole::LivePhotoImage { .. }
    ));
    assert!(matches!(
        plan.assets[1].role,
        UploadRole::LivePhotoVideo { .. }
    ));
    plan.validate()?;
    Ok(())
}

#[test]
fn migration_plan_rejects_ambiguous_live_photos_and_limits() {
    let image_id = "00000000-0000-4000-8000-000000000001";
    let missing_id = "00000000-0000-4000-8000-000000000002";
    let inventory = RemoteMigrationInventory {
        assets: vec![asset(image_id, MediaKind::Image, Some(missing_id))],
        albums: Vec::new(),
    };
    let hashes = std::iter::once((image_id.to_owned(), "d".repeat(64))).collect();
    assert!(
        assemble_migration_plan(
            inventory,
            server('a'),
            server('b'),
            &hashes,
            MigrationPlanningConfig::default(),
        )
        .is_err()
    );
    let invalid = MigrationPlanningConfig {
        inventory: MigrationListConfig::default(),
        max_asset_bytes: 2,
        max_total_bytes: 1,
    };
    assert!(invalid.validate().is_err());
}
