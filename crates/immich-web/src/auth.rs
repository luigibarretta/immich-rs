use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::str;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::profiles::ResourceIdentity;
use crate::{WebConfigError, WebLimits};

const MIN_SECRET_BYTES: usize = 16;
const MAX_SECRET_BYTES: usize = 256;
const TOKEN_BYTES: usize = 32;
const MAX_PAIRING_FAILURES: usize = 5;
const PAIRING_FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);

pub struct AuthStore {
    limits: WebLimits,
    state: Mutex<AuthState>,
}

struct AuthState {
    bootstrap: Option<Bootstrap>,
    sessions: BTreeMap<String, Session>,
}

struct Bootstrap {
    secret_digest: [u8; 32],
    cookie_token: String,
    csrf_token: String,
    expires_at: Instant,
    failures: BTreeMap<IpAddr, VecDeque<Instant>>,
}

struct Session {
    binding: [u8; 32],
    csrf_token: String,
    created_at: Instant,
    last_seen_at: Instant,
}

pub struct PairingView {
    pub cookie_token: String,
    pub csrf_token: String,
}

pub struct SessionView {
    pub binding: [u8; 32],
    pub csrf_token: String,
}

pub struct EstablishedSession {
    pub cookie_token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingFailure {
    Denied,
    InvalidCsrf,
    Expired,
    RateLimited,
    Unavailable,
}

impl AuthStore {
    pub const fn oidc(limits: WebLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(AuthState {
                bootstrap: None,
                sessions: BTreeMap::new(),
            }),
        }
    }

    pub fn load(path: &Path, limits: WebLimits) -> Result<Self, WebConfigError> {
        let mut bootstrap_material = read_secret(path)?;
        let secret_digest = Sha256::digest(&bootstrap_material).into();
        bootstrap_material.fill(0);
        let now = Instant::now();
        let expires_at = now
            .checked_add(Duration::from_secs(limits.bootstrap_lifetime_seconds))
            .ok_or_else(|| WebConfigError::new("bootstrap lifetime is invalid"))?;
        let bootstrap = Bootstrap {
            secret_digest,
            cookie_token: random_token()?,
            csrf_token: random_token()?,
            expires_at,
            failures: BTreeMap::new(),
        };
        Ok(Self {
            limits,
            state: Mutex::new(AuthState {
                bootstrap: Some(bootstrap),
                sessions: BTreeMap::new(),
            }),
        })
    }

    pub fn pairing(&self) -> Option<PairingView> {
        let now = Instant::now();
        let mut state = self.state.lock().ok()?;
        let bootstrap = state.bootstrap.as_ref()?;
        if now >= bootstrap.expires_at {
            state.bootstrap = None;
            return None;
        }
        let view = PairingView {
            cookie_token: bootstrap.cookie_token.clone(),
            csrf_token: bootstrap.csrf_token.clone(),
        };
        drop(state);
        Some(view)
    }

    pub fn pair(
        &self,
        source: IpAddr,
        cookie_token: &str,
        csrf_token: &str,
        supplied_secret: &str,
    ) -> Result<EstablishedSession, PairingFailure> {
        let now = Instant::now();
        let mut state = self.state.lock().map_err(|_| PairingFailure::Unavailable)?;
        let max_sessions = self.limits.max_sessions;
        if state.sessions.len() >= max_sessions {
            return Err(PairingFailure::Unavailable);
        }
        let bootstrap = state.bootstrap.as_mut().ok_or(PairingFailure::Expired)?;
        if now >= bootstrap.expires_at {
            state.bootstrap = None;
            return Err(PairingFailure::Expired);
        }
        let failures = bootstrap
            .failures
            .entry(source)
            .or_insert_with(|| VecDeque::with_capacity(MAX_PAIRING_FAILURES));
        retain_recent_failures(failures, now);
        if failures.len() >= MAX_PAIRING_FAILURES {
            return Err(PairingFailure::RateLimited);
        }
        if !constant_time_equal(cookie_token, &bootstrap.cookie_token)
            || !constant_time_equal(csrf_token, &bootstrap.csrf_token)
        {
            record_failure(failures, now);
            return Err(PairingFailure::InvalidCsrf);
        }
        let supplied_digest: [u8; 32] = Sha256::digest(supplied_secret.as_bytes()).into();
        if !bool::from(supplied_digest.ct_eq(&bootstrap.secret_digest)) {
            record_failure(failures, now);
            return Err(PairingFailure::Denied);
        }
        let cookie_token = random_token().map_err(|_| PairingFailure::Unavailable)?;
        let csrf_token = random_token().map_err(|_| PairingFailure::Unavailable)?;
        let binding = Sha256::digest(cookie_token.as_bytes()).into();
        state.bootstrap = None;
        state.sessions.insert(
            cookie_token.clone(),
            Session {
                binding,
                csrf_token,
                created_at: now,
                last_seen_at: now,
            },
        );
        drop(state);
        Ok(EstablishedSession { cookie_token })
    }

    pub fn authenticate(&self, cookie_token: &str) -> Option<SessionView> {
        let now = Instant::now();
        let mut state = self.state.lock().ok()?;
        expire_sessions(&mut state.sessions, now, self.limits);
        let view = {
            let session = state.sessions.get_mut(cookie_token)?;
            session.last_seen_at = now;
            SessionView {
                binding: session.binding,
                csrf_token: session.csrf_token.clone(),
            }
        };
        drop(state);
        Some(view)
    }

    pub fn establish_oidc(
        &self,
        principal: &str,
        previous_cookie: Option<&str>,
    ) -> Result<EstablishedSession, WebConfigError> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| WebConfigError::new("OIDC session state is unavailable"))?;
        expire_sessions(&mut state.sessions, now, self.limits);
        if let Some(previous) = previous_cookie {
            state.sessions.remove(previous);
        }
        if state.sessions.len() >= self.limits.max_sessions {
            return Err(WebConfigError::new("OIDC session capacity is exhausted"));
        }
        let cookie_token = random_token()?;
        let csrf_token = random_token()?;
        let mut binding_input = Vec::with_capacity(principal.len() + cookie_token.len() + 1);
        binding_input.extend_from_slice(principal.as_bytes());
        binding_input.push(0);
        binding_input.extend_from_slice(cookie_token.as_bytes());
        let binding = Sha256::digest(&binding_input).into();
        binding_input.fill(0);
        state.sessions.insert(
            cookie_token.clone(),
            Session {
                binding,
                csrf_token,
                created_at: now,
                last_seen_at: now,
            },
        );
        drop(state);
        Ok(EstablishedSession { cookie_token })
    }

    pub fn authenticate_csrf(&self, cookie_token: &str, csrf_token: &str) -> Option<SessionView> {
        let session = self.authenticate(cookie_token)?;
        constant_time_equal(csrf_token, &session.csrf_token).then_some(session)
    }

    pub fn valid_binding(&self, cookie_token: &str, binding: &[u8; 32]) -> bool {
        let now = Instant::now();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        expire_sessions(&mut state.sessions, now, self.limits);
        state
            .sessions
            .get(cookie_token)
            .is_some_and(|session| bool::from(session.binding.ct_eq(binding)))
    }

    pub fn active_session_count(&self) -> Option<usize> {
        let now = Instant::now();
        let mut state = self.state.lock().ok()?;
        expire_sessions(&mut state.sessions, now, self.limits);
        Some(state.sessions.len())
    }

    pub fn logout(&self, cookie_token: &str, csrf_token: &str) -> bool {
        let now = Instant::now();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        expire_sessions(&mut state.sessions, now, self.limits);
        let valid = state
            .sessions
            .get(cookie_token)
            .is_some_and(|session| constant_time_equal(csrf_token, &session.csrf_token));
        if valid {
            state.sessions.remove(cookie_token);
        }
        valid
    }
}

fn expire_sessions(sessions: &mut BTreeMap<String, Session>, now: Instant, limits: WebLimits) {
    let idle = Duration::from_secs(limits.session_idle_seconds);
    let absolute = Duration::from_secs(limits.session_absolute_seconds);
    sessions.retain(|_, session| {
        now.saturating_duration_since(session.last_seen_at) < idle
            && now.saturating_duration_since(session.created_at) < absolute
    });
}

fn retain_recent_failures(failures: &mut VecDeque<Instant>, now: Instant) {
    while failures
        .front()
        .is_some_and(|failure| now.saturating_duration_since(*failure) >= PAIRING_FAILURE_WINDOW)
    {
        failures.pop_front();
    }
}

fn record_failure(failures: &mut VecDeque<Instant>, now: Instant) {
    if failures.len() == MAX_PAIRING_FAILURES {
        failures.pop_front();
    }
    failures.push_back(now);
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

fn random_token() -> Result<String, WebConfigError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|_| WebConfigError::new("operating-system randomness is unavailable"))?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    bytes.fill(0);
    Ok(token)
}

fn read_secret(path: &Path) -> Result<Vec<u8>, WebConfigError> {
    let before = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot read bootstrap secret"))?;
    validate_secret_metadata(&before)?;
    let before_identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("cannot inspect bootstrap secret"))?;
    let mut file =
        File::open(path).map_err(|_| WebConfigError::new("cannot read bootstrap secret"))?;
    let opened = file
        .metadata()
        .map_err(|_| WebConfigError::new("cannot inspect bootstrap secret"))?;
    validate_secret_metadata(&opened)?;
    let opened_identity = ResourceIdentity::from_file(&file)
        .map_err(|_| WebConfigError::new("cannot inspect bootstrap secret"))?;
    let mut bootstrap_material = Vec::with_capacity(MAX_SECRET_BYTES + 1);
    file.by_ref()
        .take((MAX_SECRET_BYTES + 1) as u64)
        .read_to_end(&mut bootstrap_material)
        .map_err(|_| WebConfigError::new("cannot read bootstrap secret"))?;
    let after = fs::symlink_metadata(path)
        .map_err(|_| WebConfigError::new("cannot revalidate bootstrap secret"))?;
    validate_secret_metadata(&after)?;
    let after_identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("cannot revalidate bootstrap secret"))?;
    if opened_identity != before_identity || opened_identity != after_identity {
        bootstrap_material.fill(0);
        return Err(WebConfigError::new("bootstrap secret identity changed"));
    }
    if bootstrap_material.last() == Some(&b'\n') {
        bootstrap_material.pop();
        if bootstrap_material.last() == Some(&b'\r') {
            bootstrap_material.pop();
        }
    }
    let valid_text = str::from_utf8(&bootstrap_material)
        .is_ok_and(|value| value.trim() == value && !value.chars().any(char::is_control));
    if !(MIN_SECRET_BYTES..=MAX_SECRET_BYTES).contains(&bootstrap_material.len()) || !valid_text {
        bootstrap_material.fill(0);
        return Err(WebConfigError::new("bootstrap secret is invalid"));
    }
    Ok(bootstrap_material)
}

fn validate_secret_metadata(metadata: &Metadata) -> Result<(), WebConfigError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || !(MIN_SECRET_BYTES as u64..=MAX_SECRET_BYTES as u64 + 2).contains(&metadata.len())
        || !private_permissions(metadata)
    {
        return Err(WebConfigError::new(
            "bootstrap secret must be a private bounded regular file",
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
