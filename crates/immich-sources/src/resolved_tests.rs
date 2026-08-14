use std::fs;

use immich_rs_core::NeverCancel;

use super::{FolderScanConfig, NoProgress, scan_folder_resolved};

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resolved_scan_preserves_native_unicode_and_sidecar_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let root =
        std::env::temp_dir().join(format!("immich-rs-resolved-paths-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let _cleanup = Cleanup(root.clone());
    let media = root.join("cafe\u{301}.png");
    let sidecar = root.join("cafe\u{301}.xmp");
    fs::write(&media, b"unicode")?;
    fs::write(&sidecar, b"<xmp/>")?;
    let resolved = scan_folder_resolved(
        &root,
        "synthetic-test",
        &FolderScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )?;
    assert_eq!(resolved.plan.assets[0].relative_path, "café.png");
    assert_eq!(resolved.native_path("café.png"), Some(media.as_path()));
    assert_eq!(resolved.native_path("café.xmp"), Some(sidecar.as_path()));
    Ok(())
}
