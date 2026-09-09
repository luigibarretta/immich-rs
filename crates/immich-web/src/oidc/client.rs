use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::redirect::Policy;
use reqwest::{Client, Response};
use serde::Deserialize;
use url::{Position, Url};

use super::OidcConfig;
use super::secret::{SecretBytes, load_ca};
use super::token::{SigningKey, parse_keys};
use crate::{WebConfigError, WebLimits};

#[derive(Clone)]
pub(super) struct Provider {
    client: Client,
    authorization_endpoint: Url,
    token_endpoint: Url,
    signing_keys: Vec<SigningKey>,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    response_types_supported: Vec<String>,
    subject_types_supported: Vec<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

impl Provider {
    pub(super) async fn load(
        config: &OidcConfig,
        limits: WebLimits,
    ) -> Result<Self, WebConfigError> {
        let client = pinned_client(config, limits).await?;
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            config.issuer().trim_end_matches('/')
        );
        let discovery_bytes = fetch(&client, &discovery_url, limits.oidc_discovery_bytes).await?;
        let discovery: Discovery = serde_json::from_slice(&discovery_bytes)
            .map_err(|_| WebConfigError::new("OIDC discovery document is invalid"))?;
        validate_discovery(&discovery, config)?;
        let authorization_endpoint = endpoint(&discovery.authorization_endpoint, config)?;
        let token_endpoint = endpoint(&discovery.token_endpoint, config)?;
        let jwks_uri = endpoint(&discovery.jwks_uri, config)?;
        let jwks = fetch(&client, jwks_uri.as_str(), limits.oidc_jwks_bytes).await?;
        let signing_keys = parse_keys(&jwks, limits.oidc_signing_keys)?;
        Ok(Self {
            client,
            authorization_endpoint,
            token_endpoint,
            signing_keys,
        })
    }

    pub(super) fn authorization_url(
        &self,
        config: &OidcConfig,
        state: &str,
        nonce: &str,
        challenge: &str,
    ) -> String {
        let mut url = self.authorization_endpoint.clone();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", config.client_id())
            .append_pair("redirect_uri", config.redirect_uri())
            .append_pair("scope", "openid")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        url.into()
    }

    pub(super) async fn exchange(
        &self,
        config: &OidcConfig,
        code: &str,
        verifier: &str,
        maximum: usize,
    ) -> Result<String, WebConfigError> {
        let credential_material = SecretBytes::load(config.client_secret_file())?;
        let response = self
            .client
            .post(self.token_endpoint.clone())
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", config.redirect_uri()),
                ("client_id", config.client_id()),
                ("client_secret", credential_material.as_str()?),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .map_err(|_| WebConfigError::new("OIDC token exchange failed"))?;
        let mut bytes = bounded_response(response, maximum).await?;
        let parsed: TokenResponse = serde_json::from_slice(&bytes)
            .map_err(|_| WebConfigError::new("OIDC token response is invalid"))?;
        bytes.fill(0);
        if parsed.id_token.len() > maximum {
            return Err(WebConfigError::new("OIDC ID token is too large"));
        }
        Ok(parsed.id_token)
    }

    pub(super) fn signing_keys(&self) -> &[SigningKey] {
        &self.signing_keys
    }
}

async fn pinned_client(config: &OidcConfig, limits: WebLimits) -> Result<Client, WebConfigError> {
    let issuer =
        Url::parse(config.issuer()).map_err(|_| WebConfigError::new("OIDC issuer is invalid"))?;
    let host = issuer
        .host_str()
        .ok_or_else(|| WebConfigError::new("OIDC issuer host is unavailable"))?;
    let port = issuer
        .port_or_known_default()
        .ok_or_else(|| WebConfigError::new("OIDC issuer port is unavailable"))?;
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| WebConfigError::new("OIDC DNS resolution failed"))?;
    let mut addresses = BTreeSet::<SocketAddr>::new();
    for address in resolved.take(limits.dns_addresses + 1) {
        if !config.address_allowed(address.ip()) {
            return Err(WebConfigError::new("OIDC resolved address is not allowed"));
        }
        addresses.insert(address);
    }
    if addresses.is_empty() || addresses.len() > limits.dns_addresses {
        return Err(WebConfigError::new("OIDC DNS answer limit exceeded"));
    }
    let addresses: Vec<_> = addresses.into_iter().collect();
    let mut builder = Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .timeout(Duration::from_secs(limits.response_seconds))
        .resolve_to_addrs(host, &addresses);
    if let Some(path) = config.ca_certificate_file() {
        let pem = load_ca(path)?;
        let certificate = reqwest::Certificate::from_pem(&pem)
            .map_err(|_| WebConfigError::new("OIDC CA certificate is invalid"))?;
        builder = builder.add_root_certificate(certificate);
    }
    builder
        .build()
        .map_err(|_| WebConfigError::new("OIDC client construction failed"))
}

async fn fetch(client: &Client, url: &str, maximum: usize) -> Result<Vec<u8>, WebConfigError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| WebConfigError::new("OIDC provider request failed"))?;
    bounded_response(response, maximum).await
}

async fn bounded_response(response: Response, maximum: usize) -> Result<Vec<u8>, WebConfigError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
    {
        return Err(WebConfigError::new("OIDC provider response was refused"));
    }
    let mut body = Vec::with_capacity(maximum.min(8 * 1024));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| WebConfigError::new("OIDC provider response failed"))?;
        if body.len().saturating_add(chunk.len()) > maximum {
            return Err(WebConfigError::new("OIDC provider response is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn validate_discovery(value: &Discovery, config: &OidcConfig) -> Result<(), WebConfigError> {
    let valid = value.issuer == config.issuer()
        && value
            .response_types_supported
            .iter()
            .any(|item| item == "code")
        && !value.subject_types_supported.is_empty()
        && value
            .id_token_signing_alg_values_supported
            .iter()
            .any(|item| item == "EdDSA")
        && value
            .code_challenge_methods_supported
            .iter()
            .any(|item| item == "S256");
    valid
        .then_some(())
        .ok_or_else(|| WebConfigError::new("OIDC discovery policy is incompatible"))
}

fn endpoint(value: &str, config: &OidcConfig) -> Result<Url, WebConfigError> {
    let issuer =
        Url::parse(config.issuer()).map_err(|_| WebConfigError::new("OIDC issuer is invalid"))?;
    let url = Url::parse(value).map_err(|_| WebConfigError::new("OIDC endpoint is invalid"))?;
    let same_origin = url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url[Position::BeforeHost..Position::AfterPort]
            == issuer[Position::BeforeHost..Position::AfterPort];
    same_origin
        .then_some(url)
        .ok_or_else(|| WebConfigError::new("OIDC endpoint origin is invalid"))
}
