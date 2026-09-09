use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature;
use serde::Deserialize;
use subtle::ConstantTimeEq;

use super::{OidcConfig, valid_text};
use crate::WebConfigError;

const MAX_KEY_ID_BYTES: usize = 256;
const MAX_ROLES: usize = 64;
const MAX_AUDIENCES: usize = 8;

#[derive(Clone)]
pub(super) struct SigningKey {
    id: String,
    bytes: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    crv: String,
    x: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
    typ: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: Option<u64>,
    nbf: Option<u64>,
    nonce: String,
    azp: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

pub(super) fn parse_keys(bytes: &[u8], maximum: usize) -> Result<Vec<SigningKey>, WebConfigError> {
    let jwks: Jwks =
        serde_json::from_slice(bytes).map_err(|_| WebConfigError::new("OIDC JWKS is invalid"))?;
    if jwks.keys.is_empty() || jwks.keys.len() > maximum {
        return Err(WebConfigError::new("OIDC JWKS key count is invalid"));
    }
    let mut ids = BTreeSet::new();
    let mut keys = Vec::with_capacity(jwks.keys.len());
    for key in jwks.keys {
        let operations_valid = key
            .key_ops
            .as_ref()
            .is_none_or(|ops| ops.len() == 1 && ops.first().is_some_and(|op| op == "verify"));
        if !valid_text(&key.kid, MAX_KEY_ID_BYTES)
            || !ids.insert(key.kid.clone())
            || key.kty != "OKP"
            || key.crv != "Ed25519"
            || key.alg.as_deref().is_some_and(|value| value != "EdDSA")
            || key.usage.as_deref().is_some_and(|value| value != "sig")
            || !operations_valid
        {
            return Err(WebConfigError::new("OIDC signing key policy is invalid"));
        }
        let decoded = canonical_decode(&key.x)?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| WebConfigError::new("OIDC signing key length is invalid"))?;
        keys.push(SigningKey { id: key.kid, bytes });
    }
    Ok(keys)
}

pub(super) fn validate(
    token: &str,
    keys: &[SigningKey],
    config: &OidcConfig,
    nonce: &str,
) -> Result<String, WebConfigError> {
    let mut segments = token.split('.');
    let encoded_header = segments
        .next()
        .ok_or_else(|| WebConfigError::new("OIDC ID token is invalid"))?;
    let encoded_claims = segments
        .next()
        .ok_or_else(|| WebConfigError::new("OIDC ID token is invalid"))?;
    let encoded_signature = segments
        .next()
        .ok_or_else(|| WebConfigError::new("OIDC ID token is invalid"))?;
    if segments.next().is_some() {
        return Err(WebConfigError::new("OIDC ID token is invalid"));
    }
    let header: Header = serde_json::from_slice(&canonical_decode(encoded_header)?)
        .map_err(|_| WebConfigError::new("OIDC ID token header is invalid"))?;
    if header.alg != "EdDSA" || header.typ.as_deref().is_some_and(|value| value != "JWT") {
        return Err(WebConfigError::new("OIDC ID token algorithm is invalid"));
    }
    let key = keys
        .iter()
        .find(|key| key.id == header.kid)
        .ok_or_else(|| WebConfigError::new("OIDC ID token key is unavailable"))?;
    let signature = canonical_decode(encoded_signature)?;
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    signature::UnparsedPublicKey::new(&signature::ED25519, key.bytes)
        .verify(signing_input.as_bytes(), &signature)
        .map_err(|_| WebConfigError::new("OIDC ID token signature is invalid"))?;
    let claims: Claims = serde_json::from_slice(&canonical_decode(encoded_claims)?)
        .map_err(|_| WebConfigError::new("OIDC ID token claims are invalid"))?;
    validate_claims(&claims, config, nonce)?;
    Ok(claims.sub)
}

fn validate_claims(
    claims: &Claims,
    config: &OidcConfig,
    nonce: &str,
) -> Result<(), WebConfigError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WebConfigError::new("system clock is invalid"))?
        .as_secs();
    let audience_valid = match &claims.aud {
        Audience::One(value) => value == config.client_id(),
        Audience::Many(values) => {
            !values.is_empty()
                && values.len() <= MAX_AUDIENCES
                && values.iter().any(|value| value == config.client_id())
                && claims.azp.as_deref() == Some(config.client_id())
        }
    };
    let timing_valid = claims.exp > now
        && claims
            .iat
            .is_none_or(|issued| issued <= now.saturating_add(60))
        && claims.nbf.is_none_or(|not_before| not_before <= now);
    let roles_valid =
        claims.roles.len() <= MAX_ROLES && claims.roles.iter().all(|role| valid_text(role, 128));
    let valid = claims.iss == config.issuer()
        && valid_text(&claims.sub, 256)
        && audience_valid
        && timing_valid
        && nonce.len() == claims.nonce.len()
        && bool::from(nonce.as_bytes().ct_eq(claims.nonce.as_bytes()))
        && roles_valid
        && config.subject_allowed(&claims.sub, &claims.roles);
    valid
        .then_some(())
        .ok_or_else(|| WebConfigError::new("OIDC ID token claims were denied"))
}

fn canonical_decode(value: &str) -> Result<Vec<u8>, WebConfigError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| WebConfigError::new("OIDC base64 value is invalid"))?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(WebConfigError::new("OIDC base64 value is not canonical"));
    }
    Ok(decoded)
}
