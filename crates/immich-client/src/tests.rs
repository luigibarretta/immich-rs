use std::time::Duration;

use immich_rs_core::CancellationToken;

use crate::{
    ApiKey, ClientConfig, ClientError, ClientErrorClass, EndpointAccess, EndpointError,
    ImmichEndpoint, ImmichReadClient,
};

#[test]
fn secrets_and_origins_are_redacted() -> Result<(), Box<dyn std::error::Error>> {
    let key_text = "synthetic-secret-never-log";
    let key = ApiKey::new(key_text)?;
    let endpoint = ImmichEndpoint::parse("http://127.0.0.1:31337")?;
    let client = ImmichReadClient::new(endpoint, key.clone(), ClientConfig::default())?;

    assert!(!format!("{key:?}").contains(key_text));
    assert!(!format!("{client:?}").contains("31337"));
    Ok(())
}

#[test]
fn endpoint_policy_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    assert!(ImmichEndpoint::parse("http://127.0.0.1:2283").is_ok());
    assert!(ImmichEndpoint::parse("http://localhost:2283").is_ok());
    assert!(matches!(
        ImmichEndpoint::parse("http://example.invalid"),
        Err(EndpointError::InsecureRemoteOrigin)
    ));
    assert!(matches!(
        ImmichEndpoint::parse("https://user@example.invalid"),
        Err(EndpointError::InvalidOrigin)
    ));
    let remote = ImmichEndpoint::parse("https://example.invalid")?;
    assert!(EndpointAccess::disposable(&remote).is_err());
    assert!(EndpointAccess::production_read(&remote, false).is_err());
    assert!(EndpointAccess::production_read(&remote, true).is_ok());
    let loopback = ImmichEndpoint::parse("https://127.0.0.1:2283")?;
    assert!(EndpointAccess::production_read(&loopback, true).is_err());
    Ok(())
}

#[test]
fn production_read_cannot_be_upgraded_to_upload() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ImmichEndpoint::parse("https://example.invalid")?;
    let key = ApiKey::new("synthetic-test-key")?;
    let client =
        ImmichReadClient::new_production_read(endpoint, key, ClientConfig::default(), true)?;
    let error = client
        .authorize_upload(crate::NegotiatedServer::synthetic_for_test())
        .err()
        .ok_or("production read unexpectedly became upload")?;
    assert_eq!(error.class(), ClientErrorClass::Compatibility);
    Ok(())
}

#[test]
fn retry_contract_is_explicit() {
    let retryable = ClientError::response(
        ClientErrorClass::RateLimited,
        429,
        Some(Duration::from_secs(2)),
    );
    let permanent = ClientError::new(ClientErrorClass::Authentication);
    assert!(retryable.is_retryable());
    assert!(!permanent.is_retryable());
}

#[tokio::test]
async fn cancelled_negotiation_never_reaches_network() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ImmichEndpoint::parse("http://127.0.0.1:9")?;
    let key = ApiKey::new("synthetic-test-key")?;
    let client = ImmichReadClient::new(endpoint, key, ClientConfig::default())?;
    let cancellation = CancellationToken::default();
    cancellation.cancel();

    let error = client
        .probe(&cancellation)
        .await
        .err()
        .ok_or("missing error")?;
    assert_eq!(error.class(), ClientErrorClass::Cancelled);
    Ok(())
}
