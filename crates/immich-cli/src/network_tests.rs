use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::read_api_key_file;

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "immich-rs-cli-api-key-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn key(&self, contents: &str, mode: u32) -> Result<PathBuf, std::io::Error> {
        let path = self.0.join("api-key");
        fs::write(&path, contents)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn private_key_file_is_read_and_trimmed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new("private")?;
    let path = workspace.key("synthetic-key\r\n", 0o400)?;
    assert_eq!(
        read_api_key_file(&path).ok().as_deref(),
        Some("synthetic-key")
    );
    Ok(())
}

#[test]
fn group_or_world_readable_key_file_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new("shared")?;
    for mode in [0o640, 0o644, 0o604] {
        let path = workspace.key("synthetic-key\n", mode)?;
        assert!(read_api_key_file(&path).is_err(), "mode {mode:o}");
    }
    Ok(())
}

#[test]
fn symlinked_empty_or_oversized_key_file_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new("shape")?;
    let target = workspace.key("synthetic-key\n", 0o600)?;
    let link = workspace.0.join("link");
    std::os::unix::fs::symlink(&target, &link)?;
    assert!(read_api_key_file(&link).is_err());

    let empty = workspace.0.join("empty");
    fs::write(&empty, "")?;
    fs::set_permissions(&empty, fs::Permissions::from_mode(0o600))?;
    assert!(read_api_key_file(&empty).is_err());

    let oversized = workspace.0.join("oversized");
    fs::write(&oversized, "k".repeat(4_098))?;
    fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600))?;
    assert!(read_api_key_file(&oversized).is_err());
    Ok(())
}
