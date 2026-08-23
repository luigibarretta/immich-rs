use std::fmt::{self, Debug, Formatter};

use crate::{ClientError, ClientErrorClass};

const MAX_PEM_BUNDLE_BYTES: usize = 1024 * 1024;
const MAX_ROOT_CERTIFICATES: usize = 64;

/// Bounded custom CA bundle used with normal TLS hostname and certificate verification.
pub struct TlsRootCertificates {
    pub(crate) certificates: Vec<reqwest::Certificate>,
}

impl TlsRootCertificates {
    /// Parse a bounded PEM certificate bundle without enabling insecure TLS behavior.
    pub fn from_pem_bundle(pem: &[u8]) -> Result<Self, ClientError> {
        if pem.is_empty() || pem.len() > MAX_PEM_BUNDLE_BYTES {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        let certificates = reqwest::Certificate::from_pem_bundle(pem)
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        if certificates.is_empty() || certificates.len() > MAX_ROOT_CERTIFICATES {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        Ok(Self { certificates })
    }
}

impl Debug for TlsRootCertificates {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsRootCertificates")
            .field("certificates", &self.certificates.len())
            .finish()
    }
}
