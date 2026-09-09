use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

use immich_rs_application::{ImmichEndpoint, PinnedEndpointAddresses};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{update_path_digest, validate_id};
use crate::WebConfigError;

const MAX_ADDRESS_RANGES: usize = 32;

/// Explicit operator-selected server access mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServerMode {
    Disposable,
    ProductionRead,
}

/// Validated server profile whose origin and secret paths remain server-side.
pub struct ServerProfile {
    id: String,
    origin: String,
    api_key_file: PathBuf,
    ca_certificate_file: Option<PathBuf>,
    mode: ServerMode,
    address_ranges: Vec<AddressRange>,
    generation: u64,
    generation_sha256: String,
    credential_generation: u64,
}

impl ServerProfile {
    pub(crate) fn from_raw(raw: RawServerProfile) -> Result<Self, WebConfigError> {
        validate_id(&raw.id)?;
        let endpoint = ImmichEndpoint::parse(&raw.origin)
            .map_err(|_| WebConfigError::new("server profile origin is invalid"))?;
        let url = Url::parse(&raw.origin)
            .map_err(|_| WebConfigError::new("server profile origin is invalid"))?;
        let address_ranges = parse_ranges(&raw.allowed_cidrs)?;
        if !raw.api_key_file.is_absolute()
            || raw
                .ca_certificate_file
                .as_ref()
                .is_some_and(|path| !path.is_absolute())
            || raw.generation == 0
            || raw.credential_generation == 0
            || !valid_mode(raw.mode, &url, &endpoint, &address_ranges)
        {
            return Err(WebConfigError::new("server profile is invalid"));
        }
        let generation_sha256 = server_generation(&raw, &address_ranges);
        Ok(Self {
            id: raw.id,
            origin: raw.origin,
            api_key_file: raw.api_key_file,
            ca_certificate_file: raw.ca_certificate_file,
            mode: raw.mode,
            address_ranges,
            generation: raw.generation,
            generation_sha256,
            credential_generation: raw.credential_generation,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    #[must_use]
    pub fn api_key_file(&self) -> &Path {
        &self.api_key_file
    }

    #[must_use]
    pub fn ca_certificate_file(&self) -> Option<&Path> {
        self.ca_certificate_file.as_deref()
    }

    #[must_use]
    pub const fn mode(&self) -> ServerMode {
        self.mode
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Digest binding the operator generation to all non-secret profile inputs.
    #[must_use]
    pub fn generation_sha256(&self) -> &str {
        &self.generation_sha256
    }

    #[must_use]
    pub const fn credential_generation(&self) -> u64 {
        self.credential_generation
    }

    pub fn resolve_addresses(
        &self,
        maximum: usize,
    ) -> Result<PinnedEndpointAddresses, WebConfigError> {
        if !(1..=8).contains(&maximum) {
            return Err(WebConfigError::new("DNS address limit is invalid"));
        }
        let endpoint = ImmichEndpoint::parse(&self.origin)
            .map_err(|_| WebConfigError::new("server profile origin changed"))?;
        let url = Url::parse(&self.origin)
            .map_err(|_| WebConfigError::new("server profile origin changed"))?;
        let host = url
            .host_str()
            .ok_or_else(|| WebConfigError::new("server profile host is unavailable"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| WebConfigError::new("server profile port is unavailable"))?;
        let resolved = (host, port)
            .to_socket_addrs()
            .map_err(|_| WebConfigError::new("server profile DNS resolution failed"))?
            .take(maximum.saturating_add(1))
            .collect::<Vec<_>>();
        let addresses = validate_addresses(self.mode, &self.address_ranges, maximum, resolved)?;
        PinnedEndpointAddresses::new(&endpoint, addresses)
            .map_err(|_| WebConfigError::new("server profile address pinning failed"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawServerProfile {
    pub id: String,
    origin: String,
    api_key_file: PathBuf,
    ca_certificate_file: Option<PathBuf>,
    mode: ServerMode,
    #[serde(default)]
    allowed_cidrs: Vec<String>,
    generation: u64,
    credential_generation: u64,
}

fn valid_mode(
    mode: ServerMode,
    url: &Url,
    endpoint: &ImmichEndpoint,
    ranges: &[AddressRange],
) -> bool {
    match mode {
        ServerMode::Disposable => {
            endpoint.is_loopback()
                && url.host().is_some_and(|host| match host {
                    Host::Ipv4(address) => address.is_loopback(),
                    Host::Ipv6(address) => address.is_loopback(),
                    Host::Domain(_) => false,
                })
                && ranges.is_empty()
        }
        ServerMode::ProductionRead => {
            url.scheme() == "https" && !endpoint.is_loopback() && !ranges.is_empty()
        }
    }
}

fn parse_ranges(values: &[String]) -> Result<Vec<AddressRange>, WebConfigError> {
    if values.len() > MAX_ADDRESS_RANGES {
        return Err(WebConfigError::new("server address allowlist is too large"));
    }
    values
        .iter()
        .map(|value| AddressRange::parse(value))
        .collect()
}

fn server_generation(raw: &RawServerProfile, ranges: &[AddressRange]) -> String {
    let mut digest = Sha256::new();
    digest.update(raw.generation.to_le_bytes());
    digest.update(raw.id.as_bytes());
    digest.update(raw.origin.as_bytes());
    update_path_digest(&mut digest, &raw.api_key_file);
    if let Some(path) = &raw.ca_certificate_file {
        update_path_digest(&mut digest, path);
    }
    digest.update([match raw.mode {
        ServerMode::Disposable => 0,
        ServerMode::ProductionRead => 1,
    }]);
    for range in ranges {
        range.update_digest(&mut digest);
    }
    format!("{:x}", digest.finalize())
}

fn validate_addresses(
    mode: ServerMode,
    ranges: &[AddressRange],
    maximum: usize,
    values: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>, WebConfigError> {
    if values.len() > maximum {
        return Err(WebConfigError::new("server DNS answer limit exceeded"));
    }
    let addresses = values.into_iter().collect::<BTreeSet<_>>();
    let valid = !addresses.is_empty()
        && addresses.len() <= maximum
        && addresses.iter().all(|address| match mode {
            ServerMode::Disposable => address.ip().is_loopback(),
            ServerMode::ProductionRead => ranges.iter().any(|range| range.contains(address.ip())),
        });
    valid
        .then(|| addresses.into_iter().collect())
        .ok_or_else(|| WebConfigError::new("server resolved address is not allowed"))
}

#[derive(Clone, Copy)]
enum AddressRange {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl AddressRange {
    fn parse(value: &str) -> Result<Self, WebConfigError> {
        let (address, prefix) = value
            .split_once('/')
            .ok_or_else(|| WebConfigError::new("invalid server address allowlist"))?;
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| WebConfigError::new("invalid server address allowlist"))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| WebConfigError::new("invalid server address allowlist"))?;
        match address {
            IpAddr::V4(address) if prefix <= 32 => Ok(Self::V4 {
                network: u32::from(address) & mask_v4(prefix),
                prefix,
            }),
            IpAddr::V6(address) if prefix <= 128 => Ok(Self::V6 {
                network: u128::from(address) & mask_v6(prefix),
                prefix,
            }),
            _ => Err(WebConfigError::new("invalid server address allowlist")),
        }
    }

    fn contains(self, address: IpAddr) -> bool {
        match (self, address) {
            (Self::V4 { network, prefix }, IpAddr::V4(address)) => {
                u32::from(address) & mask_v4(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(address)) => {
                u128::from(address) & mask_v6(prefix) == network
            }
            _ => false,
        }
    }

    fn update_digest(self, digest: &mut Sha256) {
        match self {
            Self::V4 { network, prefix } => {
                digest.update([4, prefix]);
                digest.update(network.to_le_bytes());
            }
            Self::V6 { network, prefix } => {
                digest.update([6, prefix]);
                digest.update(network.to_le_bytes());
            }
        }
    }
}

const fn mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

const fn mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges_allow_exact_policy_and_deny_mixed_rebinding()
    -> Result<(), Box<dyn std::error::Error>> {
        let ranges = parse_ranges(&["fd42::/64".to_owned()])?;
        let allowed = vec!["[fd42::5]:443".parse()?, "[fd42::6]:443".parse()?];
        assert_eq!(
            validate_addresses(ServerMode::ProductionRead, &ranges, 4, allowed)?.len(),
            2
        );
        let mixed = vec!["[fd42::5]:443".parse()?, "192.0.2.8:443".parse()?];
        assert!(validate_addresses(ServerMode::ProductionRead, &ranges, 4, mixed).is_err());
        Ok(())
    }

    #[test]
    fn zero_prefixes_match_their_address_family() -> Result<(), Box<dyn std::error::Error>> {
        assert!(AddressRange::parse("0.0.0.0/0")?.contains("203.0.113.8".parse()?));
        assert!(AddressRange::parse("::/0")?.contains("2001:db8::5".parse()?));
        assert!(!AddressRange::parse("192.0.2.0/24")?.contains("::1".parse()?));
        Ok(())
    }
}
