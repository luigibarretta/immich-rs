use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NeverCancel, SourceKind, rule_id};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::{AlbumMode, NoProgress, PicasaScanConfig, scan_picasa_inputs};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-picasa-source-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("Trips/Synthetic"))?;
        fs::write(
            root.join("Trips/Synthetic/20240102-030405-image.jpg"),
            b"image",
        )?;
        fs::write(root.join("Trips/Synthetic/pair.mov"), b"motion")?;
        fs::write(root.join("Trips/Synthetic/pair.png"), b"still")?;
        fs::write(root.join("Trips/Synthetic/pair.xmp"), b"<xmp/>")?;
        fs::write(
            root.join("Trips/Synthetic/.picasa.ini"),
            b"[Picasa]\nname=Synthetic Picasa Album\n[20240102-030405-image.jpg]\ncaption=Synthetic caption\n[pair.png]\nstar=yes\n",
        )?;
        Ok(Self(root))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn archive(&self) -> Result<PathBuf, Box<dyn Error>> {
        let destination = self.0.join("picasa.zip");
        let mut writer = ZipWriter::new(File::create(&destination)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for name in [
            "Trips/Synthetic/.picasa.ini",
            "Trips/Synthetic/20240102-030405-image.jpg",
            "Trips/Synthetic/pair.mov",
            "Trips/Synthetic/pair.png",
            "Trips/Synthetic/pair.xmp",
        ] {
            writer.start_file(name, options)?;
            let bytes = fs::read(self.0.join(name))?;
            writer.write_all(&bytes)?;
        }
        writer.finish()?;
        Ok(destination)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

fn scan(
    inputs: &[PathBuf],
    config: &PicasaScanConfig,
) -> Result<immich_rs_core::NormalizedPlan, Box<dyn Error>> {
    Ok(scan_picasa_inputs(
        inputs,
        "synthetic-picasa",
        config,
        &NeverCancel,
        &mut NoProgress,
    )?)
}

#[test]
fn directory_and_zip_are_deterministic_and_equivalent() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let config = PicasaScanConfig {
        album_mode: AlbumMode::Path,
        ..PicasaScanConfig::default()
    };
    let directory = scan(&[fixture.path().to_path_buf()], &config)?;
    let archived = scan(&[fixture.archive()?], &config)?;
    assert_eq!(directory, archived);
    assert_eq!(directory.source.kind, SourceKind::Picasa);
    assert_eq!(directory.summary.assets, 3);
    let dated = directory
        .assets
        .iter()
        .find(|asset| asset.relative_path.contains("20240102"))
        .ok_or("dated asset missing")?;
    let metadata = dated
        .normalized_metadata
        .as_ref()
        .ok_or("Picasa metadata missing")?;
    assert_eq!(metadata.description.as_deref(), Some("Synthetic caption"));
    assert_eq!(
        metadata.taken_at_utc.as_deref(),
        Some("2024-01-02T03:04:05Z")
    );
    assert_eq!(
        metadata.albums,
        ["Synthetic Picasa Album", "Trips / Synthetic"]
    );
    assert!(
        directory
            .assets
            .iter()
            .filter(|asset| asset.live_photo.is_some())
            .count()
            == 2
    );
    Ok(())
}

#[test]
fn malformed_ini_is_an_explainable_plan_error() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fs::write(
        fixture.path().join("Trips/Synthetic/.picasa.ini"),
        b"[Picasa]\nname=First\nname=Second\n",
    )?;
    let plan = scan(
        &[fixture.path().to_path_buf()],
        &PicasaScanConfig::default(),
    )?;
    assert!(
        plan.errors
            .iter()
            .any(|item| item.rule_id == rule_id::PICASA_INI_INVALID)
    );
    Ok(())
}
