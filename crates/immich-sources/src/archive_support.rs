use std::io::Read;
use std::path::Component;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::Cancellation;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use time::{Date, Month, PrimitiveDateTime, Time};
use unicode_normalization::UnicodeNormalization as _;
use zip::CompressionMethod;
use zip::read::ZipFile;

use crate::ScanError;

pub fn portable_entry_path<R: Read>(
    entry: &ZipFile<'_, R>,
    max_path_bytes: usize,
) -> Result<String, ScanError> {
    if entry.name().contains(['\\', '\0'])
        || entry
            .name()
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(ScanError::InvalidArchive("unsafe ZIP entry path"));
    }
    let enclosed = entry
        .enclosed_name()
        .ok_or(ScanError::InvalidArchive("unsafe ZIP entry path"))?;
    let mut components = Vec::new();
    for component in enclosed.components() {
        let Component::Normal(value) = component else {
            return Err(ScanError::InvalidArchive("unsafe ZIP entry path"));
        };
        let value = value
            .to_str()
            .ok_or(ScanError::InvalidArchive("non-Unicode ZIP entry path"))?;
        components.push(value.nfc().collect::<String>());
    }
    let path = components.join("/");
    if path.is_empty() || path.len() > max_path_bytes {
        return Err(ScanError::LimitExceeded("max_path_bytes"));
    }
    Ok(path)
}

pub fn validate_entry<R: Read>(
    entry: &ZipFile<'_, R>,
    max_entry_bytes: u64,
    max_compression_ratio: u64,
    ratio_grace_bytes: u64,
) -> Result<(), ScanError> {
    if entry.encrypted() || entry.is_symlink() || !entry.is_file() {
        return Err(ScanError::InvalidArchive(
            "encrypted or non-regular ZIP entry",
        ));
    }
    if !matches!(
        entry.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(ScanError::InvalidArchive(
            "unsupported ZIP compression method",
        ));
    }
    if entry.size() > max_entry_bytes {
        return Err(ScanError::LimitExceeded("max_archive_entry_bytes"));
    }
    if entry.size() > ratio_grace_bytes
        && (entry.compressed_size() == 0
            || entry.size() / entry.compressed_size() > max_compression_ratio)
    {
        return Err(ScanError::InvalidArchive(
            "ZIP entry compression ratio exceeded",
        ));
    }
    Ok(())
}

pub fn stream_entry<R: Read>(
    entry: &mut ZipFile<'_, R>,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<(u64, String, String), ScanError> {
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut digest = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut byte_len = 0_u64;
    loop {
        check_cancelled(cancellation)?;
        let count = entry
            .read(&mut buffer)
            .map_err(|_| ScanError::InvalidArchive("cannot read or verify ZIP entry"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        sha1.update(&buffer[..count]);
        byte_len = byte_len.saturating_add(count as u64);
    }
    if byte_len != entry.size() {
        return Err(ScanError::InvalidArchive("ZIP entry size mismatch"));
    }
    Ok((
        byte_len,
        format!("{:x}", digest.finalize()),
        STANDARD.encode(sha1.finalize()),
    ))
}

pub fn zip_unix_ms(value: Option<zip::DateTime>) -> Option<i64> {
    let value = value?;
    let month = Month::try_from(value.month()).ok()?;
    let date = Date::from_calendar_date(i32::from(value.year()), month, value.day()).ok()?;
    let time = Time::from_hms(value.hour(), value.minute(), value.second()).ok()?;
    let nanos = PrimitiveDateTime::new(date, time)
        .assume_utc()
        .unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000).ok()
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ScanError> {
    if cancellation.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}
