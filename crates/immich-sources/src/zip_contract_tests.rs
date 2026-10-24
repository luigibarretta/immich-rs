//! ZIP reading contract: round trips, entry-name decoding and fail-closed
//! handling of hostile entries, independent of the zip crate's major version.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_core::{NeverCancel, NormalizedPlan};
use sha2::{Digest, Sha256};
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

use crate::{NoProgress, ScanError, TakeoutScanConfig, scan_google_takeout_inputs, zip_entry_name};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ALBUM: &str = "Takeout/Google Photos/Photos from 2024/";
const LANGUAGE_ENCODING_FLAG: u32 = 1 << 11;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-zip-contract-{name}-{}-{sequence}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn archive(
        &self,
        name: &str,
        entries: &[(&str, &[u8], SimpleFileOptions)],
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.0.join(name);
        let mut writer = ZipWriter::new(File::create(&path)?);
        for (entry_path, content, options) in entries {
            writer.start_file(*entry_path, *options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn scan(path: &Path) -> Result<NormalizedPlan, ScanError> {
    scan_google_takeout_inputs(
        &[path.to_path_buf()],
        "synthetic-zip-contract",
        &TakeoutScanConfig::default(),
        &NeverCancel,
        &mut NoProgress,
    )
}

/// Replace every occurrence of `from` (the local and central name copies).
fn replace_bytes(path: &Path, from: &[u8], to: &[u8]) -> TestResult {
    assert_eq!(from.len(), to.len());
    let mut bytes = fs::read(path)?;
    let mut replaced = 0;
    let mut start = 0;
    while let Some(position) = bytes[start..].windows(from.len()).position(|w| w == from) {
        let offset = start + position;
        bytes[offset..offset + to.len()].copy_from_slice(to);
        start = offset + to.len();
        replaced += 1;
    }
    assert_eq!(replaced, 2, "expected one local and one central name");
    fs::write(path, bytes)?;
    Ok(())
}

/// Rewrite one little-endian field of the first local and central header.
fn patch_headers(path: &Path, field: HeaderField, value: &dyn Fn(u32) -> u32) -> TestResult {
    let mut bytes = fs::read(path)?;
    for (signature, offset) in [(b"PK\x03\x04", field.local), (b"PK\x01\x02", field.central)] {
        let start = bytes
            .windows(4)
            .position(|window| window == signature)
            .ok_or("missing ZIP header")?
            + offset;
        let mut current = [0_u8; 4];
        current[..field.width].copy_from_slice(&bytes[start..start + field.width]);
        let updated = value(u32::from_le_bytes(current)).to_le_bytes();
        bytes[start..start + field.width].copy_from_slice(&updated[..field.width]);
    }
    fs::write(path, bytes)?;
    Ok(())
}

/// Offsets of one field in the local and central headers, and its width.
#[derive(Clone, Copy)]
struct HeaderField {
    local: usize,
    central: usize,
    width: usize,
}

const FLAGS: HeaderField = HeaderField {
    local: 6,
    central: 8,
    width: 2,
};
const UNCOMPRESSED_SIZE: HeaderField = HeaderField {
    local: 22,
    central: 24,
    width: 4,
};

fn asset_paths(plan: &NormalizedPlan) -> Vec<&str> {
    plan.assets
        .iter()
        .map(|asset| asset.relative_path.as_str())
        .collect()
}

#[test]
fn stored_deflated_and_zip64_entries_round_trip_exactly() -> TestResult {
    let workspace = Workspace::new("round-trip")?;
    let stored = b"synthetic-stored-content".as_slice();
    let deflated_content = vec![7_u8; 64 * 1_024];
    let zip64 = b"synthetic-zip64-content".as_slice();
    let archive = workspace.archive(
        "round-trip.zip",
        &[
            (
                &format!("{ALBUM}stored.png"),
                stored,
                SimpleFileOptions::default(),
            ),
            (
                &format!("{ALBUM}deflated.png"),
                &deflated_content,
                deflated(),
            ),
            (
                &format!("{ALBUM}zip64.png"),
                zip64,
                deflated().large_file(true),
            ),
        ],
    )?;
    let plan = scan(&archive)?;
    assert!(plan.errors.is_empty());
    for (name, content) in [
        ("stored.png", stored),
        ("deflated.png", deflated_content.as_slice()),
        ("zip64.png", zip64),
    ] {
        let path = format!("{ALBUM}{name}");
        let asset = plan
            .assets
            .iter()
            .find(|asset| asset.relative_path == path)
            .ok_or("missing round-trip asset")?;
        assert_eq!(asset.byte_len, u64::try_from(content.len())?);
        assert_eq!(
            asset.content_sha256,
            format!("{:x}", Sha256::digest(content))
        );
    }
    Ok(())
}

#[test]
fn traversal_absolute_backslash_and_dot_entries_fail_closed() -> TestResult {
    let workspace = Workspace::new("hostile-names")?;
    for (index, hostile) in [
        "../escape.png".to_owned(),
        format!("{ALBUM}../../../escape.png"),
        format!("{ALBUM}./alpha.png"),
        format!("{ALBUM}sub\\..\\..\\escape.png"),
        format!("..\\{ALBUM}escape.png"),
    ]
    .iter()
    .enumerate()
    {
        let archive = workspace.archive(
            &format!("hostile-{index}.zip"),
            &[(hostile, b"synthetic", deflated())],
        )?;
        assert!(
            matches!(scan(&archive), Err(ScanError::InvalidArchive(_))),
            "{hostile} must be rejected"
        );
    }
    let nul = workspace.archive("nul.zip", &[(&format!("{ALBUM}aXb.png"), b"x", deflated())])?;
    replace_bytes(&nul, b"aXb.png", b"a\0b.png")?;
    assert!(matches!(scan(&nul), Err(ScanError::InvalidArchive(_))));
    Ok(())
}

#[test]
fn a_rooted_entry_keeps_its_enclosed_relative_path() -> TestResult {
    let workspace = Workspace::new("rooted")?;
    let path = format!("{ALBUM}rooted.png");
    let archive = workspace.archive("rooted.zip", &[(&format!("_{path}"), b"x", deflated())])?;
    replace_bytes(
        &archive,
        format!("_{path}").as_bytes(),
        format!("/{path}").as_bytes(),
    )?;
    assert_eq!(asset_paths(&scan(&archive)?), [path.as_str()]);
    Ok(())
}

#[test]
fn symlink_and_size_mismatched_entries_fail_closed() -> TestResult {
    let workspace = Workspace::new("entries")?;
    let path = workspace.0.join("symlink.zip");
    let mut writer = ZipWriter::new(File::create(&path)?);
    writer.add_symlink(format!("{ALBUM}link.png"), "/etc/passwd", deflated())?;
    writer.finish()?;
    assert!(matches!(scan(&path), Err(ScanError::InvalidArchive(_))));

    let content = b"synthetic-size-content".as_slice();
    for (name, change) in [("larger.zip", 1_i32), ("smaller.zip", -1)] {
        let archive =
            workspace.archive(name, &[(&format!("{ALBUM}sized.png"), content, deflated())])?;
        patch_headers(&archive, UNCOMPRESSED_SIZE, &|size| {
            size.saturating_add_signed(change)
        })?;
        assert!(
            matches!(scan(&archive), Err(ScanError::InvalidArchive(_))),
            "{name} must be rejected"
        );
    }
    Ok(())
}

#[test]
fn unflagged_names_decode_as_code_page_437_and_flagged_names_as_utf8() -> TestResult {
    let workspace = Workspace::new("names")?;
    let cases: [(&[u8], bool, &str); 4] = [
        (b"caf\x82.png", false, "café.png"),
        ("café.png".as_bytes(), false, "caf├⌐.png"),
        ("café.png".as_bytes(), true, "café.png"),
        (b"caf\xff.png", true, "caf\u{fffd}.png"),
    ];
    for (index, (raw, flagged, expected)) in cases.into_iter().enumerate() {
        let placeholder = "#".repeat(raw.len());
        let archive = workspace.archive(
            &format!("names-{index}.zip"),
            &[(&format!("{ALBUM}{placeholder}"), b"synthetic", deflated())],
        )?;
        replace_bytes(&archive, placeholder.as_bytes(), raw)?;
        let flag = if flagged { LANGUAGE_ENCODING_FLAG } else { 0 };
        patch_headers(&archive, FLAGS, &|flags| {
            (flags & !LANGUAGE_ENCODING_FLAG) | flag
        })?;
        assert_eq!(
            asset_paths(&scan(&archive)?),
            [format!("{ALBUM}{expected}").as_str()]
        );
    }
    Ok(())
}

#[test]
fn the_code_page_437_table_matches_the_zip_crate() -> TestResult {
    let workspace = Workspace::new("cp437")?;
    let raw: Vec<u8> = (0x80..=0xff).collect();
    let placeholder = "#".repeat(raw.len());
    let archive = workspace.archive("cp437.zip", &[(&placeholder, b"x", deflated())])?;
    replace_bytes(&archive, placeholder.as_bytes(), &raw)?;
    let mut reader = ZipArchive::new(File::open(&archive)?)?;
    let entry = reader.by_index(0)?;
    // These bytes are not UTF-8, so the zip crate itself decodes them as CP437.
    assert_eq!(zip_entry_name(&entry), entry.name()?);
    assert_eq!(zip_entry_name(&entry).chars().count(), 128);
    Ok(())
}
