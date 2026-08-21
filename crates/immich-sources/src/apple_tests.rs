use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{
    CancellationToken, NORMALIZED_PLAN_SCHEMA_VERSION_V3, NeverCancel, SourceKind, rule_id,
};

use super::{AlbumMode, ApplePhotosScanConfig, NoProgress, ScanError, scan_apple_photos_inputs};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-apple-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        Ok(())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn scan(
    root: &TestDirectory,
    config: &ApplePhotosScanConfig,
) -> Result<immich_rs_core::NormalizedPlan, ScanError> {
    scan_apple_photos_inputs(
        std::slice::from_ref(&root.0),
        "synthetic-apple",
        config,
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn directory_scan_preserves_variants_and_skips_known_noise()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("preserve")?;
    directory.write("Album/original.jpg", b"original")?;
    directory.write("Album/original-edited.jpg", b"edited")?;
    directory.write("Recently Deleted/private.jpg", b"synthetic-noise")?;
    directory.write("Album/._original.jpg", b"synthetic-apple-double")?;
    let first = scan(&directory, &ApplePhotosScanConfig::default())?;
    let second = scan(&directory, &ApplePhotosScanConfig::default())?;
    assert_eq!(first, second);
    assert_eq!(first.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V3);
    assert_eq!(first.source.kind, SourceKind::ApplePhotos);
    assert_eq!(first.summary.assets, 2);
    assert_eq!(first.summary.bytes_read, 14);
    assert!(
        first
            .warnings
            .iter()
            .any(|item| item.rule_id == rule_id::APPLE_EXPORT_NOISE)
    );
    Ok(())
}

#[test]
fn xmp_live_photo_and_explicit_path_album_are_stable() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("metadata")?;
    directory.write("Trips/Synthetic/pair.heic", b"image")?;
    directory.write("Trips/Synthetic/pair.mov", b"motion")?;
    directory.write("Trips/Synthetic/pair.xmp", b"<xmp/>")?;
    let config = ApplePhotosScanConfig {
        album_mode: AlbumMode::Path,
        album_path_joiner: " - ".to_owned(),
        ..ApplePhotosScanConfig::default()
    };
    let plan = scan(&directory, &config)?;
    assert!(plan.assets.iter().all(|asset| asset.live_photo.is_some()));
    let image = plan
        .assets
        .iter()
        .find(|asset| {
            std::path::Path::new(&asset.relative_path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("heic"))
        })
        .ok_or("missing synthetic image")?;
    assert_eq!(image.metadata.len(), 1);
    assert_eq!(
        image.normalized_metadata.as_ref().map(|item| &item.albums),
        Some(&vec!["Trips - Synthetic".to_owned()])
    );
    Ok(())
}

#[test]
fn album_modes_and_joiner_validation_are_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("albums")?;
    directory.write("Outer/Inner/image.png", b"image")?;
    let folder = scan(
        &directory,
        &ApplePhotosScanConfig {
            album_mode: AlbumMode::Folder,
            ..ApplePhotosScanConfig::default()
        },
    )?;
    assert_eq!(
        folder.assets[0]
            .normalized_metadata
            .as_ref()
            .map(|metadata| &metadata.albums),
        Some(&vec!["Inner".to_owned()])
    );
    let invalid = ApplePhotosScanConfig {
        album_path_joiner: "/".to_owned(),
        ..ApplePhotosScanConfig::default()
    };
    assert!(matches!(
        scan(&directory, &invalid),
        Err(ScanError::InvalidConfiguration(_))
    ));
    Ok(())
}

#[test]
fn cancellation_returns_no_partial_apple_plan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("cancel")?;
    directory.write("image.png", b"image")?;
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let result = scan_apple_photos_inputs(
        std::slice::from_ref(&directory.0),
        "synthetic-cancelled",
        &ApplePhotosScanConfig::default(),
        &cancellation,
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)));
    Ok(())
}
