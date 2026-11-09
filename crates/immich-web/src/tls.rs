use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;

use crate::oidc::LanConfig;
use crate::{WebConfigError, bounded_file};

const MAX_TLS_FILE_BYTES: u64 = 1024 * 1024;
const MAX_CERTIFICATES: usize = 16;

pub fn load_acceptor(config: &LanConfig) -> Result<TlsAcceptor, WebConfigError> {
    let certificate_bytes = bounded_file::read_regular(
        config.tls_certificate_file(),
        MAX_TLS_FILE_BYTES,
        WebConfigError::new("TLS file must be a bounded regular file"),
    )?;
    let mut key_bytes = bounded_file::read_private(
        config.tls_private_key_file(),
        MAX_TLS_FILE_BYTES,
        WebConfigError::new("TLS private key file must be a private bounded regular file"),
    )?;
    let result = acceptor(&certificate_bytes, &key_bytes);
    key_bytes.fill(0);
    result
}

fn acceptor(certificate_bytes: &[u8], key_bytes: &[u8]) -> Result<TlsAcceptor, WebConfigError> {
    let certificates = CertificateDer::pem_slice_iter(certificate_bytes)
        .take(MAX_CERTIFICATES + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| WebConfigError::new("TLS certificate file is invalid"))?;
    let mut keys = PrivateKeyDer::pem_slice_iter(key_bytes).take(2);
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
