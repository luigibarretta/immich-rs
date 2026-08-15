use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{CancellationToken, NeverCancel, SourceKind, rule_id};

use super::{FolderScanConfig, NoProgress, ScanError, scan_google_takeout};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-takeout-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(path.join("Takeout/Google Photos/Photos from 2024"))?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        fs::write(
            self.0
                .join("Takeout/Google Photos/Photos from 2024")
                .join(name),
            content,
        )?;
        Ok(())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn scan(root: &Path) -> Result<immich_rs_core::NormalizedPlan, ScanError> {
    scan_google_takeout(
        root,
        "synthetic-takeout",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

#[test]
fn title_selects_one_same_directory_media_file() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("title")?;
    directory.write("pixel.png", b"synthetic-image")?;
    directory.write("metadata.json", br#"{"title":"pixel.png"}"#)?;
    let plan = scan(&directory.0)?;
    assert_eq!(plan.source.kind, SourceKind::GoogleTakeout);
    assert_eq!(plan.assets.len(), 1);
    assert_eq!(plan.assets[0].metadata.len(), 1);
    assert_eq!(
        plan.assets[0].metadata[0].rule_id,
        rule_id::GOOGLE_TAKEOUT_TITLE
    );
    Ok(())
}

#[test]
fn malformed_and_oversized_json_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let malformed = TestDirectory::new("malformed")?;
    malformed.write("pixel.png", b"synthetic-image")?;
    malformed.write("pixel.png.json", b"not-json")?;
    let malformed_plan = scan(&malformed.0)?;
    assert!(
        malformed_plan
            .errors
            .iter()
            .any(|error| error.rule_id == rule_id::GOOGLE_TAKEOUT_JSON_INVALID)
    );

    let oversized = TestDirectory::new("oversized")?;
    oversized.write("pixel.png", b"synthetic-image")?;
    oversized.write("pixel.png.json", &vec![b'x'; 256 * 1_024 + 1])?;
    let oversized_plan = scan(&oversized.0)?;
    assert!(
        oversized_plan
            .errors
            .iter()
            .any(|error| error.rule_id == rule_id::GOOGLE_TAKEOUT_JSON_OVERSIZED)
    );
    Ok(())
}

#[test]
fn media_without_json_is_never_silent() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unmatched-media")?;
    directory.write("pixel.png", b"synthetic-image")?;
    let plan = scan(&directory.0)?;
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.rule_id == rule_id::GOOGLE_TAKEOUT_UNMATCHED_MEDIA)
    );
    Ok(())
}

#[test]
fn duplicate_sidecars_are_ambiguous_and_never_attached() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("ambiguous")?;
    directory.write("pixel.png", b"synthetic-image")?;
    directory.write("first.json", br#"{"title":"pixel.png"}"#)?;
    directory.write("second.json", br#"{"title":"pixel.png"}"#)?;
    let plan = scan(&directory.0)?;
    assert!(plan.assets[0].metadata.is_empty());
    assert!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::GOOGLE_TAKEOUT_AMBIGUOUS)
    );
    Ok(())
}

#[test]
fn unchanged_input_is_deterministic_and_cancellation_returns_no_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let reverse = TestDirectory::new("deterministic-reverse")?;
    reverse.write("z.png", b"synthetic-z")?;
    reverse.write("z.json", br#"{"title":"z.png"}"#)?;
    reverse.write("a.png", b"synthetic-a")?;
    reverse.write("a.json", br#"{"title":"a.png"}"#)?;
    let forward = TestDirectory::new("deterministic-forward")?;
    forward.write("a.json", br#"{"title":"a.png"}"#)?;
    forward.write("a.png", b"synthetic-a")?;
    forward.write("z.json", br#"{"title":"z.png"}"#)?;
    forward.write("z.png", b"synthetic-z")?;
    assert_eq!(scan(&reverse.0)?, scan(&forward.0)?);

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let result = scan_google_takeout(
        &reverse.0,
        "synthetic-takeout",
        &FolderScanConfig::default(),
        &cancellation,
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)));
    Ok(())
}

#[test]
fn missing_takeout_layout_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("layout")?;
    fs::remove_dir_all(directory.0.join("Takeout"))?;
    let result = scan(&directory.0);
    assert!(matches!(result, Err(ScanError::UnsupportedLayout(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_takeout_layout_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = TestDirectory::new("symlinked-layout")?;
    let takeout = directory.0.join("Takeout");
    let target = directory.0.join("takeout-target");
    fs::rename(&takeout, &target)?;
    symlink("takeout-target", &takeout)?;
    let result = scan(&directory.0);
    assert!(matches!(result, Err(ScanError::UnsupportedLayout(_))));
    Ok(())
}
