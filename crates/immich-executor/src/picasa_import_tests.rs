use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NeverCancel, ServerCompatibility, ServerVersion, SourceKind};
use immich_rs_sources::{NoProgress, scan_picasa_inputs_resolved};

use crate::{
    ExecutorErrorClass, PicasaImportConfig, create_picasa_upload_plan, dry_run_picasa_import,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-picasa-import-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("Album"))?;
        fs::write(
            root.join("Album/20240102-030405-image.png"),
            b"synthetic image",
        )?;
        fs::write(
            root.join("Album/.picasa.ini"),
            b"[Picasa]\nname=Synthetic Album\n[20240102-030405-image.png]\ncaption=Synthetic caption\n",
        )?;
        Ok(Self(root))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn picasa_upload_plan_and_offline_dry_run_are_exact() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let inputs = [fixture.0.clone()];
    let config = PicasaImportConfig::default();
    let resolved = scan_picasa_inputs_resolved(
        &inputs,
        "synthetic-picasa-import",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_picasa_upload_plan(&resolved, server(), &config)?;
    assert_eq!(plan.source.kind, SourceKind::Picasa);
    assert_eq!(plan.summary.operations, 1);
    assert_eq!(plan.summary.metadata_updates, 1);
    assert_eq!(plan.summary.album_creates, 1);
    assert_eq!(plan.summary.max_mutations, 4);
    let checkpoint = fixture.0.join("checkpoint.sqlite");
    let report = dry_run_picasa_import(&plan, &inputs, &checkpoint, &config, &NeverCancel)?;
    assert_eq!(report.would_upload, 1);
    assert_eq!(report.would_update_metadata, 1);
    assert_eq!(report.would_create_albums, 1);
    assert_eq!(report.would_add_album_memberships, 1);
    assert!(!checkpoint.exists());
    Ok(())
}

#[test]
fn picasa_dry_run_rejects_config_and_source_drift() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let inputs = [fixture.0.clone()];
    let config = PicasaImportConfig::default();
    let resolved = scan_picasa_inputs_resolved(
        &inputs,
        "synthetic-picasa-drift",
        &config.source,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let plan = create_picasa_upload_plan(&resolved, server(), &config)?;
    let checkpoint = fixture.0.join("checkpoint.sqlite");
    let mut changed_config = config.clone();
    changed_config.source.picasa_albums = false;
    let error = dry_run_picasa_import(&plan, &inputs, &checkpoint, &changed_config, &NeverCancel)
        .err()
        .ok_or("configuration drift was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::InvalidPlan);
    fs::write(
        fixture.0.join("Album/20240102-030405-image.png"),
        b"changed",
    )?;
    let error = dry_run_picasa_import(&plan, &inputs, &checkpoint, &config, &NeverCancel)
        .err()
        .ok_or("source drift was accepted")?;
    assert_eq!(error.class(), ExecutorErrorClass::SourceChanged);
    Ok(())
}

fn server() -> ServerCompatibility {
    ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "c".repeat(64),
    }
}
