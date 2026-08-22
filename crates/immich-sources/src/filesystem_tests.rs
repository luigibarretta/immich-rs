use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NeverCancel, rule_id};

use super::{FolderScanConfig, NoProgress, scan_folder};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-filesystem-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, relative: &str, content: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }

    fn write_distinct(
        &self,
        relative: &str,
        content: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let path = self.0.join(relative);
        let mut handle = match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(handle) => handle,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        handle.write_all(content)?;
        Ok(true)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn scan(root: &Path) -> Result<immich_rs_core::NormalizedPlan, Box<dyn std::error::Error>> {
    Ok(scan_folder(
        root,
        "synthetic-filesystem-test",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?)
}

#[cfg(unix)]
#[test]
fn unicode_normalization_collisions_follow_host_capability()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unicode-collision")?;
    directory.write("café.png", b"nfc")?;
    let distinct = directory.write_distinct("cafe\u{301}.png", b"nfd")?;
    let plan = scan(&directory.0)?;
    if distinct {
        assert!(plan.assets.is_empty());
        assert!(
            plan.errors
                .iter()
                .any(|error| error.rule_id == rule_id::UNICODE_COLLISION)
        );
    } else {
        assert_eq!(plan.assets.len(), 1);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn unicode_sidecar_collisions_follow_host_capability() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("unicode-sidecar-collision")?;
    directory.write("caf\u{e9}.png", b"image")?;
    directory.write("caf\u{e9}.png.json", b"composed")?;
    let distinct = directory.write_distinct("cafe\u{301}.png.json", b"decomposed")?;
    let plan = scan(&directory.0)?;
    if distinct {
        assert!(plan.assets[0].metadata.is_empty());
        assert!(
            plan.errors
                .iter()
                .any(|error| error.rule_id == rule_id::UNICODE_COLLISION)
        );
    } else {
        assert_eq!(plan.assets[0].metadata.len(), 1);
    }
    Ok(())
}

#[test]
fn case_collisions_and_duplicate_basenames_follow_host_capability()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("case-collision")?;
    directory.write("Case.JPG", b"upper")?;
    let distinct = directory.write_distinct("case.jpg", b"lower")?;
    directory.write("one/repeat.png", b"one")?;
    directory.write("two/repeat.png", b"two")?;
    let plan = scan(&directory.0)?;
    assert_eq!(
        plan.errors
            .iter()
            .any(|error| error.rule_id == rule_id::CASE_COLLISION),
        distinct
    );
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.rule_id == rule_id::DUPLICATE_BASENAME)
    );
    Ok(())
}
