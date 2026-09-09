use std::collections::BTreeSet;
use std::fmt::{self, Debug, Formatter};
use std::net::SocketAddr;

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};

use crate::{ClientConfig, ClientError, ClientErrorClass, ImmichEndpoint, TlsRootCertificates};

const MAX_PINNED_ADDRESSES: usize = 8;

/// Bounded DNS result pinned into one client without changing TLS hostname checks.
pub struct PinnedEndpointAddresses {
    host: String,
    addresses: Vec<SocketAddr>,
    endpoint_sha256: [u8; 32],
}

impl PinnedEndpointAddresses {
    /// Bind a non-empty, unique address set to the exact endpoint and port.
    pub fn new(endpoint: &ImmichEndpoint, addresses: Vec<SocketAddr>) -> Result<Self, ClientError> {
        let (host, port, literal_ip) = endpoint
            .resolution_target()
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        let unique = addresses.iter().copied().collect::<BTreeSet<_>>();
        let valid = (1..=MAX_PINNED_ADDRESSES).contains(&addresses.len())
            && unique.len() == addresses.len()
            && addresses.iter().all(|address| {
                address.port() == port && literal_ip.is_none_or(|literal| address.ip() == literal)
            });
        if !valid {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        Ok(Self {
            host: host.to_owned(),
            addresses,
            endpoint_sha256: Sha256::digest(endpoint.canonical_origin().as_bytes()).into(),
        })
    }

    pub(crate) fn matches(&self, endpoint: &ImmichEndpoint) -> bool {
        let digest: [u8; 32] = Sha256::digest(endpoint.canonical_origin().as_bytes()).into();
        digest == self.endpoint_sha256
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn addresses(&self) -> &[SocketAddr] {
        &self.addresses
    }
}

impl Debug for PinnedEndpointAddresses {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PinnedEndpointAddresses")
            .field("host", &"[REDACTED]")
            .field("address_count", &self.addresses.len())
            .finish_non_exhaustive()
    }
}

pub fn build_http_client(
    endpoint: &ImmichEndpoint,
    config: ClientConfig,
    roots: Option<TlsRootCertificates>,
    addresses: Option<PinnedEndpointAddresses>,
) -> Result<reqwest::Client, ClientError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let mut builder = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(config.request_timeout)
        .redirect(reqwest::redirect::Policy::none());
    if let Some(roots) = roots {
        for certificate in roots.certificates {
            builder = builder.add_root_certificate(certificate);
        }
    }
    if let Some(addresses) = addresses {
        if !addresses.matches(endpoint) {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        builder = builder.resolve_to_addrs(addresses.host(), addresses.addresses());
    }
    builder
        .build()
        .map_err(|_| ClientError::new(ClientErrorClass::Protocol))
}
