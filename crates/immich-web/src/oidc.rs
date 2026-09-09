use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use url::Url;

use crate::{WebConfigError, WebLimits};

mod client;
mod flow;
mod secret;
mod token;

pub use flow::{LoginView, OidcIdentity, OidcManager};

const MAX_SUBJECTS: usize = 64;
const MAX_SUBJECT_BYTES: usize = 256;
const MAX_ROLE_BYTES: usize = 128;
const MAX_CLIENT_ID_BYTES: usize = 256;
const MAX_ADDRESS_RANGES: usize = 32;

pub struct LanConfig {
    tls_certificate_file: PathBuf,
    tls_private_key_file: PathBuf,
    oidc: OidcConfig,
}

impl LanConfig {
    pub fn from_raw(
        raw: RawLanConfig,
        public_origin: &str,
        limits: WebLimits,
    ) -> Result<Self, WebConfigError> {
        if !raw.tls_certificate_file.is_absolute() || !raw.tls_private_key_file.is_absolute() {
            return Err(WebConfigError::new("TLS files must use absolute paths"));
        }
        Ok(Self {
            tls_certificate_file: raw.tls_certificate_file,
            tls_private_key_file: raw.tls_private_key_file,
            oidc: OidcConfig::from_raw(raw.oidc, public_origin, limits)?,
        })
    }

    pub fn tls_certificate_file(&self) -> &Path {
        &self.tls_certificate_file
    }

    pub fn tls_private_key_file(&self) -> &Path {
        &self.tls_private_key_file
    }

    pub const fn oidc(&self) -> &OidcConfig {
        &self.oidc
    }
}

pub struct OidcConfig {
    issuer: String,
    client_id: String,
    client_secret_file: PathBuf,
    ca_certificate_file: Option<PathBuf>,
    address_ranges: Vec<AddressRange>,
    allowed_subjects: Vec<String>,
    required_role: Option<String>,
    redirect_uri: String,
}

impl OidcConfig {
    fn from_raw(
        raw: RawOidcConfig,
        public_origin: &str,
        limits: WebLimits,
    ) -> Result<Self, WebConfigError> {
        let issuer = validate_issuer(&raw.issuer)?;
        let address_ranges = parse_ranges(&raw.allowed_cidrs)?;
        let valid_paths = raw.client_secret_file.is_absolute()
            && raw
                .ca_certificate_file
                .as_ref()
                .is_none_or(|path| path.is_absolute());
        let valid_client = valid_text(&raw.client_id, MAX_CLIENT_ID_BYTES);
        let valid_subjects = !raw.allowed_subjects.is_empty()
            && raw.allowed_subjects.len() <= MAX_SUBJECTS
            && raw
                .allowed_subjects
                .iter()
                .all(|value| valid_text(value, MAX_SUBJECT_BYTES));
        let valid_role = raw
            .required_role
            .as_ref()
            .is_some_and(|value| valid_text(value, MAX_ROLE_BYTES));
        if !valid_paths
            || !valid_client
            || address_ranges.is_empty()
            || !(valid_subjects ^ valid_role)
            || raw.allowed_algorithm != "EdDSA"
            || limits.oidc_state_seconds == 0
        {
            return Err(WebConfigError::new("OIDC LAN profile is invalid"));
        }
        Ok(Self {
            issuer,
            client_id: raw.client_id,
            client_secret_file: raw.client_secret_file,
            ca_certificate_file: raw.ca_certificate_file,
            address_ranges,
            allowed_subjects: raw.allowed_subjects,
            required_role: raw.required_role,
            redirect_uri: format!("{public_origin}/oidc/callback"),
        })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }

    pub fn ca_certificate_file(&self) -> Option<&Path> {
        self.ca_certificate_file.as_deref()
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn subject_allowed(&self, subject: &str, roles: &[String]) -> bool {
        self.allowed_subjects
            .iter()
            .any(|candidate| candidate == subject)
            || self
                .required_role
                .as_ref()
                .is_some_and(|required| roles.iter().any(|role| role == required))
    }

    pub fn address_allowed(&self, address: IpAddr) -> bool {
        self.address_ranges
            .iter()
            .any(|range| range.contains(address))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLanConfig {
    tls_certificate_file: PathBuf,
    tls_private_key_file: PathBuf,
    oidc: RawOidcConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOidcConfig {
    issuer: String,
    client_id: String,
    client_secret_file: PathBuf,
    ca_certificate_file: Option<PathBuf>,
    allowed_cidrs: Vec<String>,
    #[serde(default)]
    allowed_subjects: Vec<String>,
    required_role: Option<String>,
    allowed_algorithm: String,
}

fn validate_issuer(value: &str) -> Result<String, WebConfigError> {
    let url = Url::parse(value).map_err(|_| WebConfigError::new("OIDC issuer is invalid"))?;
    let valid = url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.as_str() == value;
    valid
        .then(|| value.to_owned())
        .ok_or_else(|| WebConfigError::new("OIDC issuer is invalid"))
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn parse_ranges(values: &[String]) -> Result<Vec<AddressRange>, WebConfigError> {
    if values.is_empty() || values.len() > MAX_ADDRESS_RANGES {
        return Err(WebConfigError::new("OIDC address policy is invalid"));
    }
    let mut unique = values.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != values.len() {
        return Err(WebConfigError::new("OIDC address policy is invalid"));
    }
    values
        .iter()
        .map(|value| AddressRange::parse(value))
        .collect()
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
            .ok_or_else(|| WebConfigError::new("OIDC address policy is invalid"))?;
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| WebConfigError::new("OIDC address policy is invalid"))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| WebConfigError::new("OIDC address policy is invalid"))?;
        match address {
            IpAddr::V4(address) if prefix <= 32 => Ok(Self::V4 {
                network: u32::from(address) & mask_v4(prefix),
                prefix,
            }),
            IpAddr::V6(address) if prefix <= 128 => Ok(Self::V6 {
                network: u128::from(address) & mask_v6(prefix),
                prefix,
            }),
            _ => Err(WebConfigError::new("OIDC address policy is invalid")),
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
