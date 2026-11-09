use std::path::Path;
use std::str;

use crate::{WebConfigError, bounded_file};

const MAX_SECRET_BYTES: usize = 512;
const MAX_CA_BYTES: usize = 1024 * 1024;

pub(super) struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub(super) fn load(path: &Path) -> Result<Self, WebConfigError> {
        let mut bytes = bounded_file::read_private(
            path,
            MAX_SECRET_BYTES as u64,
            WebConfigError::new("OIDC client secret file must be a private bounded regular file"),
        )?;
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
    bounded_file::read_regular(
        path,
        MAX_CA_BYTES as u64,
        WebConfigError::new("OIDC file must be a bounded regular file"),
    )
}
