use std::path::Path;

use immich_rs_application::{ApiKey, TlsRootCertificates};

use crate::{WebConfigError, bounded_file};

const MAX_API_KEY_FILE_BYTES: u64 = 4_097;
const MAX_CA_CERTIFICATE_BYTES: u64 = 1_024 * 1_024;

#[cfg(test)]
static SECRET_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(super) fn api_key(path: &Path) -> Result<ApiKey, WebConfigError> {
    count_read();
    let mut bytes = bounded_file::read_private(
        path,
        MAX_API_KEY_FILE_BYTES,
        WebConfigError::new("server secret file must be private, bounded and regular"),
    )?;
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
        count_read();
        let mut bytes = bounded_file::read_regular(
            path,
            MAX_CA_CERTIFICATE_BYTES,
            WebConfigError::new("server file must be bounded and regular"),
        )?;
        let result = TlsRootCertificates::from_pem_bundle(&bytes)
            .map_err(|_| WebConfigError::new("server CA certificate file is invalid"));
        bytes.fill(0);
        result
    })
    .transpose()
}

#[cfg(test)]
fn count_read() {
    SECRET_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(test))]
const fn count_read() {}

#[cfg(test)]
pub(super) fn reset_secret_read_count() {
    SECRET_READS.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn secret_read_count() -> usize {
    SECRET_READS.load(std::sync::atomic::Ordering::Relaxed)
}
