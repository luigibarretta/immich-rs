use base64::Engine as _;
use immich_rs_core::MediaKind;

use crate::ClientErrorClass;
use crate::migration::{migration_asset, normalized_metadata};
use crate::models::{ArchiveAssetResponse, ArchiveExifResponse, AssetTypeResponse};

fn response() -> ArchiveAssetResponse {
    ArchiveAssetResponse {
        id: "00000000-0000-4000-8000-000000000001".to_owned(),
        original_file_name: "synthetic.jpg".to_owned(),
        checksum: base64::engine::general_purpose::STANDARD.encode([7_u8; 20]),
        r#type: AssetTypeResponse::Image,
        exif_info: Some(ArchiveExifResponse {
            file_size_in_byte: Some(42),
            date_time_original: Some("2024-01-02T03:04:05.999+01:00".to_owned()),
            description: Some("synthetic description".to_owned()),
            latitude: Some(12.5),
            longitude: Some(-45.25),
        }),
        file_created_at: Some("2024-01-02T02:04:05Z".to_owned()),
        file_modified_at: Some("2024-01-02T02:05:06Z".to_owned()),
        live_photo_video_id: Some("00000000-0000-4000-8000-000000000002".to_owned()),
    }
}

#[test]
fn migration_asset_normalizes_only_the_declared_matrix() -> Result<(), Box<dyn std::error::Error>> {
    let asset = migration_asset(response())?;
    assert_eq!(asset.media_kind, MediaKind::Image);
    assert_eq!(asset.byte_len, 42);
    assert_eq!(asset.checksum_sha1, "07".repeat(20));
    assert_eq!(asset.created_at_unix_ms, 1_704_161_045_000);
    let metadata = asset
        .normalized_metadata
        .ok_or("expected synthetic metadata")?;
    assert_eq!(
        metadata.description.as_deref(),
        Some("synthetic description")
    );
    assert_eq!(
        metadata.taken_at_utc.as_deref(),
        Some("2024-01-02T02:04:05Z")
    );
    let location = metadata.location.ok_or("expected synthetic location")?;
    assert_eq!(location.latitude, "12.5");
    assert_eq!(location.longitude, "-45.25");
    assert!(metadata.albums.is_empty());
    Ok(())
}

#[test]
fn malformed_remote_facts_fail_closed() {
    let mut missing_size = response();
    if let Some(exif) = &mut missing_size.exif_info {
        exif.file_size_in_byte = None;
    }
    let error = migration_asset(missing_size).err();
    assert!(error.is_some_and(|value| value.class() == ClientErrorClass::Protocol));

    let incomplete_location = normalized_metadata(None, None, Some(1.0), None).err();
    assert!(incomplete_location.is_some_and(|value| value.class() == ClientErrorClass::Protocol));
}
