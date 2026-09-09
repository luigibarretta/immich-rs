use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Form, Query, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsAcceptor;

use super::support::TlsIdentity;

const CLIENT_ID: &str = "immich-rs-web";
const CLIENT_SECRET: &str = "synthetic-oidc-secret";
const REDIRECT_URI: &str = "https://console.example:2285/oidc/callback";

#[derive(Clone, Copy)]
pub enum ClaimsMode {
    Valid,
    WrongIssuer,
    WrongAudience,
    WrongNonce,
    Expired,
    RoleDenied,
}

pub struct DisposableIdp {
    pub issuer: String,
    pub state: Arc<IdpState>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

pub struct IdpState {
    issuer: String,
    inner: Mutex<Inner>,
}

struct Inner {
    codes: BTreeMap<String, Authorization>,
    sequence: u64,
    claims: ClaimsMode,
    signing: SigningIdentity,
    outage: bool,
}

struct Authorization {
    nonce: String,
    challenge: String,
}

struct SigningIdentity {
    id: String,
    key: Ed25519KeyPair,
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: String,
    nonce: String,
    code_challenge: String,
    code_challenge_method: String,
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    code_verifier: String,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    sub: &'static str,
    aud: &'a str,
    exp: u64,
    iat: u64,
    nonce: &'a str,
    roles: [&'static str; 1],
}

impl DisposableIdp {
    pub async fn start(identity: &TlsIdentity) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let issuer = format!("https://{address}/");
        let signing = SigningIdentity::new("key-1")?;
        let state = Arc::new(IdpState {
            issuer: issuer.clone(),
            inner: Mutex::new(Inner {
                codes: BTreeMap::new(),
                sequence: 0,
                claims: ClaimsMode::Valid,
                signing,
                outage: false,
            }),
        });
        let router = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .route("/jwks", get(jwks))
            .with_state(Arc::clone(&state));
        let acceptor = identity.acceptor()?;
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(serve(listener, router, acceptor, stopped));
        Ok(Self {
            issuer,
            state,
            stop: Some(stop),
            task: Some(task),
        })
    }

    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _send_result = stop.send(());
        }
        if let Some(task) = self.task.take() {
            let _join_result = task.await;
        }
    }
}

impl Drop for DisposableIdp {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _send_result = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl IdpState {
    pub fn set_claims(&self, value: ClaimsMode) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.claims = value;
        }
    }

    pub fn rotate(&self) -> Result<(), Box<dyn std::error::Error>> {
        let signing = SigningIdentity::new("key-2")?;
        let mut inner = self.inner.lock().map_err(|_| "IdP state unavailable")?;
        inner.signing = signing;
        drop(inner);
        Ok(())
    }

    pub fn set_outage(&self, value: bool) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.outage = value;
        }
    }
}

impl SigningIdentity {
    fn new(id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| "cannot generate IdP signing key")?;
        Ok(Self {
            id: id.to_owned(),
            key: Ed25519KeyPair::from_pkcs8(document.as_ref())
                .map_err(|_| "cannot parse IdP signing key")?,
        })
    }
}

async fn discovery(State(state): State<Arc<IdpState>>) -> Response {
    if outage(&state) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let issuer = &state.issuer;
    json(&serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}authorize"),
        "token_endpoint": format!("{issuer}token"),
        "jwks_uri": format!("{issuer}jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA"],
        "code_challenge_methods_supported": ["S256"]
    }))
    .into_response()
}

async fn authorize(
    State(state): State<Arc<IdpState>>,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    let valid = query.response_type == "code"
        && query.client_id == CLIENT_ID
        && query.redirect_uri == REDIRECT_URI
        && query.scope == "openid"
        && query.code_challenge_method == "S256";
    if !valid || outage(&state) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(mut inner) = state.inner.lock() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    inner.sequence = inner.sequence.saturating_add(1);
    let code = format!("code-{}", inner.sequence);
    inner.codes.insert(
        code.clone(),
        Authorization {
            nonce: query.nonce,
            challenge: query.code_challenge,
        },
    );
    Redirect::temporary(&format!("{REDIRECT_URI}?code={code}&state={}", query.state))
        .into_response()
}

async fn token(State(state): State<Arc<IdpState>>, Form(form): Form<TokenForm>) -> Response {
    if outage(&state) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(mut inner) = state.inner.lock() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(authorization) = inner.codes.remove(&form.code) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(form.code_verifier.as_bytes()));
    let valid = form.grant_type == "authorization_code"
        && form.redirect_uri == REDIRECT_URI
        && form.client_id == CLIENT_ID
        && form.client_secret == CLIENT_SECRET
        && challenge == authorization.challenge;
    if !valid {
        return StatusCode::BAD_REQUEST.into_response();
    }
    id_token(&state.issuer, &inner, &authorization.nonce).map_or_else(
        |_| StatusCode::SERVICE_UNAVAILABLE.into_response(),
        |value| json(&serde_json::json!({"id_token": value})),
    )
}

async fn jwks(State(state): State<Arc<IdpState>>) -> Response {
    if outage(&state) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(inner) = state.inner.lock() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let public = URL_SAFE_NO_PAD.encode(inner.signing.key.public_key().as_ref());
    json(&serde_json::json!({"keys": [{
        "kid": inner.signing.id, "kty": "OKP", "crv": "Ed25519",
        "x": public, "alg": "EdDSA", "use": "sig", "key_ops": ["verify"]
    }]}))
    .into_response()
}

fn id_token(issuer: &str, inner: &Inner, nonce: &str) -> Result<String, serde_json::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let (claim_issuer, audience, claim_nonce, expiry, roles) = match inner.claims {
        ClaimsMode::Valid => (issuer, CLIENT_ID, nonce, now + 120, ["operator"]),
        ClaimsMode::WrongIssuer => (
            "https://wrong.invalid/",
            CLIENT_ID,
            nonce,
            now + 120,
            ["operator"],
        ),
        ClaimsMode::WrongAudience => (issuer, "wrong-client", nonce, now + 120, ["operator"]),
        ClaimsMode::WrongNonce => (issuer, CLIENT_ID, "wrong-nonce", now + 120, ["operator"]),
        ClaimsMode::Expired => (
            issuer,
            CLIENT_ID,
            nonce,
            now.saturating_sub(1),
            ["operator"],
        ),
        ClaimsMode::RoleDenied => (issuer, CLIENT_ID, nonce, now + 120, ["viewer"]),
    };
    let header = serde_json::json!({"alg": "EdDSA", "kid": inner.signing.id, "typ": "JWT"});
    let claims = Claims {
        iss: claim_issuer,
        sub: "operator-1",
        aud: audience,
        exp: expiry,
        iat: now,
        nonce: claim_nonce,
        roles,
    };
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
    let input = format!("{header}.{claims}");
    let signature = URL_SAFE_NO_PAD.encode(inner.signing.key.sign(input.as_bytes()).as_ref());
    Ok(format!("{input}.{signature}"))
}

fn outage(state: &IdpState) -> bool {
    state.inner.lock().map_or(true, |inner| inner.outage)
}

fn json(value: &serde_json::Value) -> Response {
    let Ok(body) = serde_json::to_vec(value) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    ([(CONTENT_TYPE, "application/json")], body).into_response()
}

async fn serve(
    listener: TcpListener,
    router: Router,
    acceptor: TlsAcceptor,
    mut stop: oneshot::Receiver<()>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { break; };
                let service = TowerToHyperService::new(router.clone());
                let connection_acceptor = acceptor.clone();
                connections.spawn(async move {
                    if let Ok(stream) = connection_acceptor.accept(stream).await {
                        let _result = http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .await;
                    }
                });
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) { break; }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}
