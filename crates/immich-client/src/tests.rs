use std::time::Duration;

use immich_rs_core::{
    CancellationToken, ProductionWriteConfirmation, ServerCompatibility, ServerVersion, UploadPlan,
};
use sha2::{Digest, Sha256};

use crate::{
    ApiKey, ClientConfig, ClientError, ClientErrorClass, EndpointAccess, EndpointError,
    ImmichEndpoint, ImmichReadClient, TlsRootCertificates,
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
    assert!(ImmichEndpoint::parse("http://synthetic.localhost:2283").is_ok());
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
    let loopback_domain = ImmichEndpoint::parse("https://synthetic.localhost:2283")?;
    assert!(EndpointAccess::production_read(&loopback_domain, true).is_err());
    Ok(())
}

#[test]
fn production_read_cannot_be_upgraded_to_upload() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ImmichEndpoint::parse("https://example.invalid")?;
    let key = ApiKey::new("synthetic-test-key")?;
    let client =
        ImmichReadClient::new_production_read(endpoint, key, ClientConfig::default(), true)?;
    let error = client
        .authorize_upload(crate::NegotiatedServer::synthetic_for_test(
            ServerCompatibility {
                version: ServerVersion {
                    major: 3,
                    minor: 1,
                    patch: 0,
                },
                identity_sha256: "a".repeat(64),
            },
            "b".repeat(64),
        ))
        .err()
        .ok_or("production read unexpectedly became upload")?;
    assert_eq!(error.class(), ClientErrorClass::Compatibility);
    Ok(())
}

#[test]
fn production_upload_is_bound_to_plan_count_origin_and_hashed_backup()
-> Result<(), Box<dyn std::error::Error>> {
    let compatibility = ServerCompatibility {
        version: ServerVersion {
            major: 3,
            minor: 1,
            patch: 0,
        },
        identity_sha256: "a".repeat(64),
    };
    let mut plan: UploadPlan = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "normalized_plan_sha256": "b".repeat(64),
        "source": {
            "kind": "folder",
            "label": "synthetic-source",
            "fingerprint_sha256": "c".repeat(64),
            "case_sensitive": true,
            "unicode_normalization": "nfc"
        },
        "configuration_sha256": "d".repeat(64),
        "server": compatibility,
        "operations": [{
            "operation_id": "e".repeat(64),
            "relative_path": "synthetic.jpg",
            "media_kind": "image",
            "byte_len": 16,
            "content_sha256": "f".repeat(64),
            "created_at_unix_ms": 1,
            "modified_at_unix_ms": 1,
            "xmp_sidecar": null,
            "role": {"kind": "standalone"}
        }],
        "summary": {
            "operations": 1,
            "media_bytes": 16,
            "xmp_sidecars": 0,
            "live_photo_pairs": 0
        }
    }))?;
    plan.validate()?;
    let digest = crate::upload_plan_sha256(&plan)?;
    let backup_reference = "synthetic-backup-reference";
    let confirmation =
        ProductionWriteConfirmation::new(true, digest, 1, backup_reference.to_owned())?;
    let endpoint = ImmichEndpoint::parse("https://example.invalid")?;
    let origin_sha256 = format!(
        "{:x}",
        Sha256::digest(endpoint.canonical_origin().as_bytes())
    );
    let client = ImmichReadClient::new_production_read(
        endpoint,
        ApiKey::new("synthetic-test-key")?,
        ClientConfig::default(),
        true,
    )?;
    let negotiated =
        crate::NegotiatedServer::synthetic_for_test(plan.server.clone(), origin_sha256);
    let production = client.authorize_production_upload(negotiated, &plan, &confirmation)?;
    assert!(production.upload().is_production());
    assert!(production.authorization().matches_plan(&plan));
    plan.operations[0].modified_at_unix_ms = 2;
    assert!(!production.authorization().matches_plan(&plan));
    assert_ne!(
        production.authorization().backup_reference_sha256(),
        backup_reference
    );
    assert!(!format!("{production:?}").contains(backup_reference));
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

#[test]
fn custom_tls_roots_are_bounded_and_fail_closed() {
    assert!(TlsRootCertificates::from_pem_bundle(b"").is_err());
    assert!(TlsRootCertificates::from_pem_bundle(b"not a certificate").is_err());
    assert!(TlsRootCertificates::from_pem_bundle(&vec![b'x'; 1024 * 1024 + 1]).is_err());
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
