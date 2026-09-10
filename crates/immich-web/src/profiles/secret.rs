use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use immich_rs_application::{ApiKey, TlsRootCertificates};

use super::ResourceIdentity;
use crate::WebConfigError;

const MAX_API_KEY_FILE_BYTES: u64 = 4_097;
const MAX_CA_CERTIFICATE_BYTES: u64 = 1_024 * 1_024;

#[cfg(test)]
static SECRET_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(super) fn api_key(path: &Path) -> Result<ApiKey, WebConfigError> {
    let mut bytes = bounded_file(path, MAX_API_KEY_FILE_BYTES, true)?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| WebConfigError::new("server API key file is invalid"))?
        .trim_end_matches(['\r', '\n']);
    let result =
        ApiKey::new(value).map_err(|_| WebConfigError::new("server API key file is invalid"));
    bytes.fill(0);
    result
}

pub(super) fn root_certificates(
    path: Option<&Path>,
) -> Result<Option<TlsRootCertificates>, WebConfigError> {
    path.map(|path| {
        let mut bytes = bounded_file(path, MAX_CA_CERTIFICATE_BYTES, false)?;
        let result = TlsRootCertificates::from_pem_bundle(&bytes)
            .map_err(|_| WebConfigError::new("server CA certificate file is invalid"));
        bytes.fill(0);
        result
    })
    .transpose()
}

fn bounded_file(path: &Path, maximum: u64, private: bool) -> Result<Vec<u8>, WebConfigError> {
    #[cfg(test)]
    SECRET_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let before = file_stamp(path, maximum, private)?;
    let file = File::open(path).map_err(|_| WebConfigError::new("cannot open server file"))?;
    let opened = open_stamp(&file, maximum, private)?;
    if opened != before {
        return Err(WebConfigError::new("server file identity changed"));
    }
    let capacity = usize::try_from(before.length)
        .map_err(|_| WebConfigError::new("server file size is invalid"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| WebConfigError::new("cannot read server file"))?;
    if u64::try_from(bytes.len())
        .ok()
        .is_none_or(|length| length > maximum)
    {
        bytes.fill(0);
        return Err(WebConfigError::new("server file size is invalid"));
    }
    let after = file_stamp(path, maximum, private)?;
    if after != opened || u64::try_from(bytes.len()).ok() != Some(opened.length) {
        bytes.fill(0);
        return Err(WebConfigError::new("server file identity changed"));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileStamp {
    identity: ResourceIdentity,
    length: u64,
}

fn file_stamp(path: &Path, maximum: u64, private: bool) -> Result<FileStamp, WebConfigError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot inspect server file"))?;
    let identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("server file identity is unavailable"))?;
    stamp(&metadata, identity, maximum, private)
}

fn open_stamp(file: &File, maximum: u64, private: bool) -> Result<FileStamp, WebConfigError> {
    let metadata = file
        .metadata()
        .map_err(|_| WebConfigError::new("cannot inspect server file"))?;
    let identity = ResourceIdentity::from_file(file)
        .map_err(|_| WebConfigError::new("server file identity is unavailable"))?;
    stamp(&metadata, identity, maximum, private)
}

fn stamp(
    metadata: &fs::Metadata,
    identity: ResourceIdentity,
    maximum: u64,
    private: bool,
) -> Result<FileStamp, WebConfigError> {
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(WebConfigError::new(
            "server file must be bounded and regular",
        ));
    }
    if private && !private_permissions(metadata) {
        return Err(WebConfigError::new("server secret file is not private"));
    }
    Ok(FileStamp {
        identity,
        length: metadata.len(),
    })
}

#[cfg(unix)]
fn private_permissions(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(windows)]
fn private_permissions(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
pub(super) fn reset_secret_read_count() {
    SECRET_READS.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn secret_read_count() -> usize {
    SECRET_READS.load(std::sync::atomic::Ordering::Relaxed)
}
