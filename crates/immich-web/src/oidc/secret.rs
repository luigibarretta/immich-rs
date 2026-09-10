use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;
use std::str;

use crate::WebConfigError;
use crate::profiles::ResourceIdentity;

const MAX_SECRET_BYTES: usize = 512;
const MAX_CA_BYTES: usize = 1024 * 1024;

pub(super) struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub(super) fn load(path: &Path) -> Result<Self, WebConfigError> {
        let mut bytes = read_file(path, MAX_SECRET_BYTES, true)?;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let valid = (16..=MAX_SECRET_BYTES).contains(&bytes.len())
            && str::from_utf8(&bytes)
                .is_ok_and(|value| value.trim() == value && !value.chars().any(char::is_control));
        if !valid {
            bytes.fill(0);
            return Err(WebConfigError::new("OIDC client secret file is invalid"));
        }
        Ok(Self(bytes))
    }

    pub(super) fn as_str(&self) -> Result<&str, WebConfigError> {
        str::from_utf8(&self.0)
            .map_err(|_| WebConfigError::new("OIDC client secret file is invalid"))
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub(super) fn load_ca(path: &Path) -> Result<Vec<u8>, WebConfigError> {
    read_file(path, MAX_CA_BYTES, false)
}

fn read_file(path: &Path, maximum: usize, private: bool) -> Result<Vec<u8>, WebConfigError> {
    let before =
        fs::symlink_metadata(path).map_err(|_| WebConfigError::new("cannot inspect OIDC file"))?;
    validate_metadata(&before, maximum, private)?;
    let before_identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("cannot inspect OIDC file"))?;
    let mut file = File::open(path).map_err(|_| WebConfigError::new("cannot open OIDC file"))?;
    let opened = file
        .metadata()
        .map_err(|_| WebConfigError::new("cannot inspect OIDC file"))?;
    validate_metadata(&opened, maximum, private)?;
    let opened_identity = ResourceIdentity::from_file(&file)
        .map_err(|_| WebConfigError::new("cannot inspect OIDC file"))?;
    let capacity = usize::try_from(opened.len()).map_or(maximum, |length| maximum.min(length));
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| WebConfigError::new("cannot read OIDC file"))?;
    let after = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot revalidate OIDC file"))?;
    validate_metadata(&after, maximum, private)?;
    let after_identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("cannot revalidate OIDC file"))?;
    if bytes.is_empty()
        || bytes.len() > maximum
        || opened_identity != before_identity
        || opened_identity != after_identity
    {
        bytes.fill(0);
        return Err(WebConfigError::new("OIDC file identity changed"));
    }
    Ok(bytes)
}

fn validate_metadata(
    metadata: &Metadata,
    maximum: usize,
    private: bool,
) -> Result<(), WebConfigError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > maximum as u64
        || (private && !private_permissions(metadata))
    {
        return Err(WebConfigError::new(
            "OIDC file must be a bounded regular file",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn private_permissions(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(windows)]
const fn private_permissions(_metadata: &Metadata) -> bool {
    true
}
