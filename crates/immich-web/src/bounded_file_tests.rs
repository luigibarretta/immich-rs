use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{read_private, read_regular};
use crate::WebConfigError;

const ERROR: WebConfigError = WebConfigError::new("test file rejected");
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-bounded-file-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn file(&self, name: &str, bytes: &[u8], mode: u32) -> Result<PathBuf, std::io::Error> {
        let path = self.0.join(name);
        fs::write(&path, bytes)?;
        set_mode(&path, mode)?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &std::path::Path, _mode: u32) -> Result<(), std::io::Error> {
    Ok(())
}

#[test]
fn private_read_returns_exact_bytes_within_the_bound() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let path = workspace.file("secret", b"value\n", 0o600)?;
    assert_eq!(read_private(&path, 6, ERROR)?, b"value\n");
    assert_eq!(read_private(&path, 5, ERROR), Err(ERROR));
    Ok(())
}

#[test]
fn empty_and_missing_files_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let empty = workspace.file("empty", b"", 0o600)?;
    assert_eq!(read_private(&empty, 16, ERROR), Err(ERROR));
    assert_eq!(read_regular(&empty, 16, ERROR), Err(ERROR));
    let missing = workspace.0.join("missing");
    assert_eq!(read_private(&missing, 16, ERROR), Err(ERROR));
    assert_eq!(read_regular(&missing, 16, ERROR), Err(ERROR));
    Ok(())
}

#[cfg(unix)]
#[test]
fn private_read_rejects_group_or_other_access() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    for mode in [0o640, 0o604, 0o660, 0o644] {
        let path = workspace.file(&format!("secret-{mode:o}"), b"value", mode)?;
        assert_eq!(read_private(&path, 16, ERROR), Err(ERROR), "mode {mode:o}");
        assert_eq!(read_regular(&path, 16, ERROR)?, b"value");
    }
    let owner_only = workspace.file("secret-400", b"value", 0o400)?;
    assert_eq!(read_private(&owner_only, 16, ERROR)?, b"value");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_and_directories_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let target = workspace.file("target", b"value", 0o600)?;
    let link = workspace.0.join("link");
    std::os::unix::fs::symlink(&target, &link)?;
    assert_eq!(read_private(&link, 16, ERROR), Err(ERROR));
    assert_eq!(read_regular(&link, 16, ERROR), Err(ERROR));
    assert_eq!(read_private(&workspace.0, 16, ERROR), Err(ERROR));
    assert_eq!(read_regular(&workspace.0, 16, ERROR), Err(ERROR));
    Ok(())
}

#[cfg(unix)]
#[test]
fn private_permissions_matches_the_owner_only_policy() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let private = workspace.file("private", b"value", 0o600)?;
    let shared = workspace.file("shared", b"value", 0o640)?;
    assert!(super::private_permissions(&fs::symlink_metadata(private)?));
    assert!(!super::private_permissions(&fs::symlink_metadata(shared)?));
    Ok(())
}
