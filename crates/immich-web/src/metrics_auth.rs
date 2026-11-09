use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{WebConfigError, bounded_file};

const TOKEN_HEX_BYTES: usize = 64;
const MAX_ADDRESS_RANGES: usize = 32;

pub struct MetricsAuthConfig {
    bearer_token_file: PathBuf,
    address_ranges: Vec<AddressRange>,
}

pub struct MetricsAuthenticator {
    token_digest: [u8; 32],
    address_ranges: Vec<AddressRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMetricsAuthConfig {
    bearer_token_file: PathBuf,
    allowed_cidrs: Vec<String>,
}

impl MetricsAuthConfig {
    pub(super) fn from_raw(raw: RawMetricsAuthConfig) -> Result<Self, WebConfigError> {
        if !raw.bearer_token_file.is_absolute()
            || raw.allowed_cidrs.is_empty()
            || raw.allowed_cidrs.len() > MAX_ADDRESS_RANGES
        {
            return Err(WebConfigError::new(
                "metrics authentication policy is invalid",
            ));
        }
        let address_ranges = raw
            .allowed_cidrs
            .iter()
            .map(|value| AddressRange::parse(value))
            .collect::<Result<Vec<_>, _>>()?;
        if address_ranges
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != address_ranges.len()
        {
            return Err(WebConfigError::new(
                "metrics authentication policy is invalid",
            ));
        }
        Ok(Self {
            bearer_token_file: raw.bearer_token_file,
            address_ranges,
        })
    }
}

impl MetricsAuthenticator {
    pub(super) fn load(config: &MetricsAuthConfig) -> Result<Self, WebConfigError> {
        let mut token = read_token(&config.bearer_token_file)?;
        let token_digest = Sha256::digest(&token).into();
        token.fill(0);
        Ok(Self {
            token_digest,
            address_ranges: config.address_ranges.clone(),
        })
    }

    pub(super) fn authorize(&self, source: IpAddr, headers: &HeaderMap) -> bool {
        if !self
            .address_ranges
            .iter()
            .any(|range| range.contains(source))
            || headers.get_all(AUTHORIZATION).iter().count() != 1
        {
            return false;
        }
        let Some(token) = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| valid_token(value.as_bytes()))
        else {
            return false;
        };
        let supplied_digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        bool::from(supplied_digest.ct_eq(&self.token_digest))
    }
}

fn read_token(path: &Path) -> Result<Vec<u8>, WebConfigError> {
    // One optional trailing CRLF/LF on top of the fixed token length.
    let mut token = bounded_file::read_private(
        path,
        TOKEN_HEX_BYTES as u64 + 2,
        WebConfigError::new("metrics credential must be a private bounded regular file"),
    )?;
    if token.last() == Some(&b'\n') {
        token.pop();
        if token.last() == Some(&b'\r') {
            token.pop();
        }
    }
    if !valid_token(&token) {
        token.fill(0);
        return Err(WebConfigError::new("metrics credential is invalid"));
    }
    Ok(token)
}

const fn valid_token(token: &[u8]) -> bool {
    if token.len() != TOKEN_HEX_BYTES {
        return false;
    }
    let mut index = 0;
    while index < token.len() {
        if !matches!(token[index], b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        index += 1;
    }
    true
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum AddressRange {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl AddressRange {
    fn parse(value: &str) -> Result<Self, WebConfigError> {
        let (address, prefix) = value
            .split_once('/')
            .ok_or_else(|| WebConfigError::new("metrics authentication policy is invalid"))?;
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| WebConfigError::new("metrics authentication policy is invalid"))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| WebConfigError::new("metrics authentication policy is invalid"))?;
        match address {
            IpAddr::V4(address) if prefix <= 32 => Ok(Self::V4 {
                network: u32::from(address) & mask_v4(prefix),
                prefix,
            }),
            IpAddr::V6(address) if prefix <= 128 => Ok(Self::V6 {
                network: u128::from(address) & mask_v6(prefix),
                prefix,
            }),
            _ => Err(WebConfigError::new(
                "metrics authentication policy is invalid",
            )),
        }
    }

    fn contains(self, address: IpAddr) -> bool {
        match (self, address) {
            (Self::V4 { network, prefix }, IpAddr::V4(value)) => {
                u32::from(value) & mask_v4(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(value)) => {
                u128::from(value) & mask_v6(prefix) == network
            }
            _ => false,
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
#[path = "metrics_auth_tests.rs"]
mod tests;
