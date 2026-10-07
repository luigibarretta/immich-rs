use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NORMALIZED_PLAN_SCHEMA_VERSION_V2, NeverCancel, rule_id};

use super::{NoProgress, TakeoutScanConfig, scan_google_takeout_inputs};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-takeout-v2-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(path.join("Takeout/Google Photos"))?;
        Ok(Self(path))
    }

    fn write(&self, relative: &str, content: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.0.join("Takeout/Google Photos").join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn scan(root: &Path) -> Result<immich_rs_core::NormalizedPlan, super::ScanError> {
    scan_google_takeout_inputs(
        &[root.to_path_buf()],
        "synthetic-takeout-v2",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn supplemental_name_resolves_bounded_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("supplemental")?;
    directory.write("Photos from 2024/long-name.png", b"synthetic-image")?;
    directory.write(
        "Photos from 2024/long-na.supplemental-metadata.json",
        br#"{"title":"not-the-media.png","description":"synthetic description","photoTakenTime":{"timestamp":"1704067200","formatted":"ignored"},"geoData":{"latitude":1.25,"longitude":-2.5}}"#,
    )?;
    let plan = scan(&directory.0)?;
    assert_eq!(plan.schema_version, NORMALIZED_PLAN_SCHEMA_VERSION_V2);
    let asset = plan.assets.first().ok_or("missing asset")?;
    assert_eq!(
        asset.metadata[0].rule_id,
        rule_id::GOOGLE_TAKEOUT_SUPPLEMENTAL
    );
    let metadata = asset
        .normalized_metadata
        .as_ref()
        .ok_or("missing metadata")?;
    assert_eq!(
        metadata.description.as_deref(),
        Some("synthetic description")
    );
    assert_eq!(
        metadata.taken_at_utc.as_deref(),
        Some("2024-01-01T00:00:00Z")
    );
    let location = metadata.location.as_ref().ok_or("missing location")?;
    assert_eq!(
        (location.latitude.as_str(), location.longitude.as_str()),
        ("1.25", "-2.5")
    );
    plan.validate()?;
    Ok(())
}

#[test]
fn indexed_supplemental_names_select_the_matching_numbered_media()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("indexed-supplemental")?;
    directory.write("Photos from 2024/pixel.jpg", b"base-image")?;
    directory.write("Photos from 2024/pixel(1).jpg", b"numbered-image")?;
    directory.write(
        "Photos from 2024/pixel.jpg.supplemental-metadata.json",
        br#"{"title":"pixel.jpg","description":"base"}"#,
    )?;
    directory.write(
        "Photos from 2024/pixel.jpg.supplemental-metadata(1).json",
        br#"{"title":"pixel.jpg","description":"numbered"}"#,
    )?;

    let plan = scan(&directory.0)?;
    assert!(plan.errors.is_empty());
    assert_eq!(plan.assets.len(), 2);
    let base = plan
        .assets
        .iter()
        .find(|asset| asset.relative_path.ends_with("/pixel.jpg"))
        .ok_or("missing base asset")?;
    let numbered = plan
        .assets
        .iter()
        .find(|asset| asset.relative_path.ends_with("/pixel(1).jpg"))
        .ok_or("missing numbered asset")?;
    assert_eq!(
        base.normalized_metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("base")
    );
    assert_eq!(
        numbered
            .normalized_metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("numbered")
    );
    Ok(())
}

#[test]
fn truncated_and_legacy_indexed_sidecars_select_the_numbered_media()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("truncated-indexed-supplemental")?;
    for (stem, truncated) in [("long", "supplemen"), ("short", "s")] {
        directory.write(
            &format!("Photos from 2024/{stem}.jpg"),
            format!("{stem}-base").as_bytes(),
        )?;
        directory.write(
            &format!("Photos from 2024/{stem}(1).jpg"),
            format!("{stem}-numbered").as_bytes(),
        )?;
        directory.write(
            &format!("Photos from 2024/{stem}.jpg.supplemental-metadata.json"),
            format!(r#"{{"title":"{stem}.jpg","description":"base"}}"#).as_bytes(),
        )?;
        directory.write(
            &format!("Photos from 2024/{stem}.jpg.{truncated}(1).json"),
            format!(r#"{{"title":"{stem}.jpg","description":"numbered"}}"#).as_bytes(),
        )?;
    }
    directory.write("Photos from 2024/legacy.jpg", b"legacy-base")?;
    directory.write("Photos from 2024/legacy(2).jpg", b"legacy-numbered")?;
    directory.write(
        "Photos from 2024/legacy.jpg.json",
        br#"{"title":"legacy.jpg","description":"base"}"#,
    )?;
    directory.write(
        "Photos from 2024/legacy.jpg(2).json",
        br#"{"title":"legacy.jpg","description":"numbered"}"#,
    )?;

    let plan = scan(&directory.0)?;
    assert!(plan.errors.is_empty(), "{:?}", plan.errors);
    assert_eq!(plan.assets.len(), 6);
    for asset in &plan.assets {
        let expected = if asset.relative_path.contains('(') {
            "numbered"
        } else {
            "base"
        };
        assert_eq!(
            asset
                .normalized_metadata
                .as_ref()
                .and_then(|metadata| metadata.description.as_deref()),
            Some(expected),
            "{}",
            asset.relative_path
        );
    }
    Ok(())
}

#[test]
fn empty_album_title_uses_the_album_directory_name() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("empty-album-title")?;
    let media = b"synthetic-identical-image";
    directory.write("Photos from 2024/pixel.png", media)?;
    directory.write(
        "Photos from 2024/pixel.png.json",
        br#"{"title":"pixel.png"}"#,
    )?;
    directory.write("Family Album/pixel.png", media)?;
    directory.write("Family Album/pixel.png.json", br#"{"title":"pixel.png"}"#)?;
    directory.write("Family Album/metadata.json", br#"{"title":""}"#)?;

    let plan = scan(&directory.0)?;
    assert!(plan.errors.is_empty());
    let metadata = plan.assets[0]
        .normalized_metadata
        .as_ref()
        .ok_or("missing normalized metadata")?;
    assert_eq!(metadata.albums, ["Family Album"]);
    Ok(())
}

#[test]
fn album_alias_collapses_to_the_year_copy() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("album-alias")?;
    let media = b"synthetic-identical-image";
    let sidecar = br#"{"title":"pixel.png","description":"synthetic description","photoTakenTime":{"timestamp":"1704153600"}}"#;
    directory.write("Photos from 2024/pixel.png", media)?;
    directory.write("Photos from 2024/pixel.png.json", sidecar)?;
    directory.write("Synthetic Album/pixel.png", media)?;
    directory.write("Synthetic Album/pixel.png.json", sidecar)?;
    directory.write(
        "Synthetic Album/metadata.json",
        br#"{"title":"Synthetic Album"}"#,
    )?;

    let plan = scan(&directory.0)?;
    assert_eq!(plan.assets.len(), 1);
    assert_eq!(
        plan.assets[0].relative_path,
        "Takeout/Google Photos/Photos from 2024/pixel.png"
    );
    let metadata = plan.assets[0]
        .normalized_metadata
        .as_ref()
        .ok_or("missing metadata")?;
    assert_eq!(metadata.albums, ["Synthetic Album"]);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.rule_id == rule_id::GOOGLE_TAKEOUT_CONTENT_ALIAS)
    );
    assert!(plan.errors.is_empty());
    plan.validate()?;
    Ok(())
}

#[test]
fn conflicting_alias_metadata_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("alias-conflict")?;
    directory.write("Photos from 2024/pixel.png", b"synthetic-identical-image")?;
    directory.write(
        "Photos from 2024/pixel.png.json",
        br#"{"title":"pixel.png","description":"first synthetic value"}"#,
    )?;
    directory.write("Synthetic Album/pixel.png", b"synthetic-identical-image")?;
    directory.write(
        "Synthetic Album/pixel.png.json",
        br#"{"title":"pixel.png","description":"second synthetic value"}"#,
    )?;
    directory.write(
        "Synthetic Album/metadata.json",
        br#"{"title":"Synthetic Album"}"#,
    )?;

    let plan = scan(&directory.0)?;
    assert_eq!(plan.assets.len(), 1);
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::GOOGLE_TAKEOUT_METADATA_CONFLICT)
    );
    Ok(())
}
