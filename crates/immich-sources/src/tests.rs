use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{CancellationToken, NeverCancel, ProgressEvent, rule_id};

use super::{FolderScanConfig, NoProgress, ScanError, scan_folder, scan_folder_internal};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-sources-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn write(&self, relative: &str, content: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}

fn scan(root: &Path) -> Result<immich_rs_core::NormalizedPlan, ScanError> {
    scan_folder(
        root,
        "synthetic-test",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn recursive_scan_is_deterministic_and_streamed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("deterministic")?;
    directory.write("z/video.mp4", b"synthetic-video")?;
    directory.write("a/image.png", b"synthetic-image")?;
    let first = scan(&directory.path)?;
    let second = scan(&directory.path)?;
    assert_eq!(first, second);
    assert_eq!(first.assets.len(), 2);
    assert_eq!(first.assets[0].relative_path, "a/image.png");
    assert_eq!(first.summary.bytes_read, 30);
    Ok(())
}

#[test]
fn sidecars_and_live_photos_are_explainable() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("associations")?;
    directory.write("pair.png", b"image")?;
    directory.write("pair.mov", b"motion")?;
    directory.write("pair.png.json", b"{}")?;
    directory.write("pair.xmp", b"<xmp/>")?;
    let plan = scan(&directory.path)?;
    assert_eq!(plan.summary.assets, 2);
    assert_eq!(plan.summary.sidecars, 2);
    assert!(plan.assets.iter().all(|asset| asset.live_photo.is_some()));
    let image = &plan.assets[1];
    assert_eq!(image.relative_path, "pair.png");
    assert_eq!(image.metadata.len(), 2);
    assert!(
        image
            .evidence
            .iter()
            .any(|evidence| evidence.rule_id == rule_id::LIVE_PHOTO_BASENAME)
    );
    Ok(())
}

#[test]
fn unicode_paths_are_normalized_to_nfc() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unicode")?;
    directory.write("cafe\u{301}.png", b"unicode")?;
    let plan = scan(&directory.path)?;
    assert_eq!(plan.assets[0].relative_path, "café.png");
    Ok(())
}

#[cfg(unix)]
#[test]
fn unicode_normalization_collisions_fail_closed_with_a_rule_id()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unicode-collision")?;
    directory.write("café.png", b"nfc")?;
    directory.write("cafe\u{301}.png", b"nfd")?;
    let plan = scan(&directory.path)?;
    assert!(plan.assets.is_empty());
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::UNICODE_COLLISION)
    );
    Ok(())
}

#[test]
fn overlong_paths_are_rejected_with_the_specific_limit_rule()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("path-limit")?;
    directory.write("long-name.png", b"bounded")?;
    let config = FolderScanConfig {
        max_path_bytes: 64,
        ..FolderScanConfig::default()
    };
    let long_directory = "nested".repeat(12);
    directory.write(&format!("{long_directory}/image.png"), b"over-limit")?;
    let plan = scan_folder(
        &directory.path,
        "synthetic-test",
        &config,
        &NeverCancel,
        &mut NoProgress,
    )?;
    assert_eq!(plan.assets.len(), 1);
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::PATH_LIMIT_EXCEEDED)
    );
    Ok(())
}

#[test]
fn collisions_and_duplicate_basenames_are_deterministic() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new("collisions")?;
    directory.write("Case.JPG", b"upper")?;
    directory.write("case.jpg", b"lower")?;
    directory.write("one/repeat.png", b"one")?;
    directory.write("two/repeat.png", b"two")?;
    let plan = scan(&directory.path)?;
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::CASE_COLLISION)
    );
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.rule_id == rule_id::DUPLICATE_BASENAME)
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_are_never_followed() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = TestDirectory::new("symlink")?;
    directory.write("target.png", b"target")?;
    symlink("target.png", directory.path.join("alias.png"))?;
    let plan = scan(&directory.path)?;
    assert_eq!(plan.assets.len(), 1);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.rule_id == rule_id::SYMLINK_SKIPPED)
    );
    Ok(())
}

#[test]
fn cancellation_returns_no_partial_plan() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("cancel")?;
    directory.write("image.png", b"content")?;
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let result = scan_folder(
        &directory.path,
        "synthetic-test",
        &FolderScanConfig::default(),
        &cancellation,
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)));
    Ok(())
}

#[test]
fn cancellation_during_discovery_returns_no_partial_plan() -> Result<(), Box<dyn std::error::Error>>
{
    struct CancelAfterFirst {
        token: CancellationToken,
    }

    impl super::ProgressObserver for CancelAfterFirst {
        fn observe(&mut self, event: ProgressEvent) {
            if event.sequence == 1 {
                self.token.cancel();
            }
        }
    }

    let directory = TestDirectory::new("cancel-during-discovery")?;
    directory.write("a.png", b"first")?;
    directory.write("b.png", b"second")?;
    let cancellation = CancellationToken::default();
    let mut observer = CancelAfterFirst {
        token: cancellation.clone(),
    };
    let result = scan_folder(
        &directory.path,
        "synthetic-test",
        &FolderScanConfig::default(),
        &cancellation,
        &mut observer,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)));
    Ok(())
}

#[test]
fn entry_limit_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("limit")?;
    directory.write("one.png", b"one")?;
    directory.write("two.png", b"two")?;
    let config = FolderScanConfig {
        max_entries: 1,
        ..FolderScanConfig::default()
    };
    let result = scan_folder(
        &directory.path,
        "synthetic-test",
        &config,
        &NeverCancel,
        &mut NoProgress,
    );
    assert!(matches!(
        result,
        Err(ScanError::LimitExceeded("max_entries"))
    ));
    Ok(())
}

#[test]
fn source_change_during_read_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("changed")?;
    directory.write("image.png", b"before")?;
    let changed_path = directory.path.join("image.png");
    let mut changed = false;
    let plan = scan_folder_internal(
        &directory.path,
        "synthetic-test",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
        &mut |path| {
            if path == changed_path && !changed {
                let _ignored = fs::write(path, b"content-after-change");
                changed = true;
            }
        },
    )?;
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::SOURCE_CHANGED)
    );
    Ok(())
}

#[test]
fn property_creation_order_and_buffer_size_do_not_change_the_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let names = [
        "tree/a.png",
        "tree/b.png",
        "tree/c.png",
        "tree/d.png",
        "other/e.png",
        "other/f.png",
        "other/g.png",
        "other/h.png",
    ];
    let mut reference = None;
    for seed in 1_u64..=8 {
        let directory = TestDirectory::new("property")?;
        let mut ranked = names
            .iter()
            .map(|name| {
                let rank = name.bytes().fold(seed, |value, byte| {
                    value
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(u64::from(byte) + 1)
                });
                (rank, *name)
            })
            .collect::<Vec<_>>();
        ranked.sort_unstable();
        for (_, name) in ranked {
            directory.write(name, name.as_bytes())?;
        }
        let config = FolderScanConfig {
            buffer_bytes: if seed % 2 == 0 { 4_096 } else { 128 * 1_024 },
            ..FolderScanConfig::default()
        };
        let plan = scan_folder(
            &directory.path,
            "synthetic-property",
            &config,
            &NeverCancel,
            &mut NoProgress,
        )?;
        if let Some(expected) = &reference {
            assert_eq!(&plan, expected);
        } else {
            reference = Some(plan);
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_media_becomes_an_explainable_error() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDirectory::new("unreadable")?;
    directory.write("locked.png", b"locked")?;
    let path = directory.path.join("locked.png");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
    let plan = scan(&directory.path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::UNREADABLE_FILE)
    );
    assert!(plan.assets.is_empty());
    Ok(())
}
