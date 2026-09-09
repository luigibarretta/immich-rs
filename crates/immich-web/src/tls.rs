use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;

use crate::WebConfigError;
use crate::oidc::LanConfig;

const MAX_TLS_FILE_BYTES: usize = 1024 * 1024;
const MAX_CERTIFICATES: usize = 16;

pub fn load_acceptor(config: &LanConfig) -> Result<TlsAcceptor, WebConfigError> {
    let certificate_bytes = read_file(config.tls_certificate_file(), false)?;
    let key_bytes = read_file(config.tls_private_key_file(), true)?;
    let certificates = CertificateDer::pem_slice_iter(&certificate_bytes)
        .take(MAX_CERTIFICATES + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| WebConfigError::new("TLS certificate file is invalid"))?;
    let mut keys = PrivateKeyDer::pem_slice_iter(&key_bytes).take(2);
    let key = keys
        .next()
        .transpose()
        .map_err(|_| WebConfigError::new("TLS private key file is invalid"))?
        .ok_or_else(|| WebConfigError::new("TLS private key file is invalid"))?;
    if certificates.is_empty() || certificates.len() > MAX_CERTIFICATES || keys.next().is_some() {
        return Err(WebConfigError::new("TLS identity file is invalid"));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| WebConfigError::new("TLS 1.3 configuration is unavailable"))?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|_| WebConfigError::new("TLS identity does not match"))?;
    Ok(TlsAcceptor::from(Arc::new(server)))
}

fn read_file(path: &Path, private: bool) -> Result<Vec<u8>, WebConfigError> {
    let before =
        fs::symlink_metadata(path).map_err(|_| WebConfigError::new("cannot inspect TLS file"))?;
    validate_metadata(&before, private)?;
    let mut file = File::open(path).map_err(|_| WebConfigError::new("cannot open TLS file"))?;
    let opened = file
        .metadata()
        .map_err(|_| WebConfigError::new("cannot inspect TLS file"))?;
    validate_metadata(&opened, private)?;
    let capacity = usize::try_from(opened.len())
        .map_or(MAX_TLS_FILE_BYTES, |length| MAX_TLS_FILE_BYTES.min(length));
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take((MAX_TLS_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| WebConfigError::new("cannot read TLS file"))?;
    let after = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot revalidate TLS file"))?;
    if bytes.is_empty()
        || bytes.len() > MAX_TLS_FILE_BYTES
        || !same_file(&before, &opened)
        || !same_file(&opened, &after)
    {
        bytes.fill(0);
        return Err(WebConfigError::new("TLS file identity changed"));
    }
    Ok(bytes)
}

fn validate_metadata(metadata: &Metadata, private: bool) -> Result<(), WebConfigError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_TLS_FILE_BYTES as u64
        || (private && !private_permissions(metadata))
    {
        return Err(WebConfigError::new(
            "TLS file must be a bounded regular file",
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

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    left.volume_serial_number() == right.volume_serial_number()
        && left.file_index() == right.file_index()
}
