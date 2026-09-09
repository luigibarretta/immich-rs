use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::client::Provider;
use super::token;
use crate::{WebConfig, WebConfigError};

const TOKEN_BYTES: usize = 32;
const MAX_CODE_BYTES: usize = 4 * 1024;

pub struct OidcManager {
    config: Arc<WebConfig>,
    login_cookie: String,
    login_csrf: String,
    pending: Mutex<BTreeMap<String, Pending>>,
}

struct Pending {
    peer: IpAddr,
    nonce: String,
    verifier: String,
    created_at: Instant,
    previous_cookie: Option<String>,
}

pub struct LoginView {
    pub cookie_token: String,
    pub csrf_token: String,
}

pub struct OidcIdentity {
    pub principal: String,
    pub previous_cookie: Option<String>,
}

impl OidcManager {
    pub fn new(config: Arc<WebConfig>) -> Result<Self, WebConfigError> {
        if config.lan().is_none() {
            return Err(WebConfigError::new("OIDC requires LAN mode"));
        }
        Ok(Self {
            config,
            login_cookie: random_token()?,
            login_csrf: random_token()?,
            pending: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn login_view(&self) -> LoginView {
        LoginView {
            cookie_token: self.login_cookie.clone(),
            csrf_token: self.login_csrf.clone(),
        }
    }

    pub async fn begin(
        &self,
        peer: IpAddr,
        cookie: &str,
        csrf: &str,
        previous_cookie: Option<String>,
    ) -> Result<String, WebConfigError> {
        if !constant_time_equal(cookie, &self.login_cookie)
            || !constant_time_equal(csrf, &self.login_csrf)
        {
            return Err(WebConfigError::new("OIDC login CSRF validation failed"));
        }
        let oidc = self
            .config
            .lan()
            .ok_or_else(|| WebConfigError::new("OIDC configuration is unavailable"))?
            .oidc();
        let provider = Provider::load(oidc, self.config.limits()).await?;
        let state_token = random_token()?;
        let nonce = random_token()?;
        let verifier = random_token()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let now = Instant::now();
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| WebConfigError::new("OIDC state is unavailable"))?;
        purge(&mut pending, now, self.config.limits().oidc_state_seconds);
        if pending.len() >= self.config.limits().max_sessions {
            return Err(WebConfigError::new("OIDC state capacity is exhausted"));
        }
        pending.insert(
            state_token.clone(),
            Pending {
                peer,
                nonce: nonce.clone(),
                verifier,
                created_at: now,
                previous_cookie,
            },
        );
        drop(pending);
        Ok(provider.authorization_url(oidc, &state_token, &nonce, &challenge))
    }

    pub async fn finish(
        &self,
        peer: IpAddr,
        state_token: &str,
        code: &str,
    ) -> Result<OidcIdentity, WebConfigError> {
        if !valid_token(state_token)
            || code.is_empty()
            || code.len() > MAX_CODE_BYTES
            || code.chars().any(char::is_control)
        {
            return Err(WebConfigError::new("OIDC callback is invalid"));
        }
        let now = Instant::now();
        let pending = {
            let mut states = self
                .pending
                .lock()
                .map_err(|_| WebConfigError::new("OIDC state is unavailable"))?;
            purge(&mut states, now, self.config.limits().oidc_state_seconds);
            states.remove(state_token)
        }
        .ok_or_else(|| WebConfigError::new("OIDC state is invalid or expired"))?;
        if pending.peer != peer {
            return Err(WebConfigError::new("OIDC callback peer changed"));
        }
        let oidc = self
            .config
            .lan()
            .ok_or_else(|| WebConfigError::new("OIDC configuration is unavailable"))?
            .oidc();
        let provider = Provider::load(oidc, self.config.limits()).await?;
        let id_token = provider
            .exchange(
                oidc,
                code,
                &pending.verifier,
                self.config.limits().oidc_token_bytes,
            )
            .await?;
        let principal = token::validate(&id_token, provider.signing_keys(), oidc, &pending.nonce);
        let mut token_bytes = id_token.into_bytes();
        token_bytes.fill(0);
        std::hint::black_box(&token_bytes);
        Ok(OidcIdentity {
            principal: principal?,
            previous_cookie: pending.previous_cookie,
        })
    }
}

fn purge(states: &mut BTreeMap<String, Pending>, now: Instant, lifetime_seconds: u64) {
    let lifetime = Duration::from_secs(lifetime_seconds);
    states.retain(|_, value| now.saturating_duration_since(value.created_at) < lifetime);
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

fn valid_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn random_token() -> Result<String, WebConfigError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|_| WebConfigError::new("operating-system randomness is unavailable"))?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    bytes.fill(0);
    Ok(token)
}
