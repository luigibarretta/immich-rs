#![forbid(unsafe_code)]

use std::fs;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::header::LOCATION;
use axum::http::{Method, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};
use serde_json::json;

#[path = "apply_security/flow.rs"]
mod flow;
#[path = "apply_security/mock.rs"]
mod mock;
mod support;

use flow::{
    admit, apply, confirm, confirm_and_apply, confirm_values, idempotency_key, pair,
    prepare_receipt, receipt_fields, wait_for_link, wait_status,
};
use mock::start_mock;
use support::{HOST, ORIGIN, send};

#[tokio::test]
async fn exact_single_use_grant_admits_one_executor_apply() -> Result<(), Box<dyn std::error::Error>>
{
    let (origin, mock, server) = start_mock().await?;
    let fixture = prepare_receipt(&origin, None).await?;
    for (digest, count, backup) in [
        ("f".repeat(64), fixture.count.clone(), String::new()),
        (fixture.digest.clone(), "2".to_owned(), String::new()),
        (
            fixture.digest.clone(),
            fixture.count.clone(),
            "not-allowed".to_owned(),
        ),
    ] {
        let denied = confirm(&fixture, &digest, &count, &backup).await?;
        assert_eq!(denied.status, StatusCode::FORBIDDEN);
    }
    let confirmed = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
    assert_eq!(confirmed.status, StatusCode::OK);
    let key = idempotency_key(&confirmed)?;
    let apply_path = format!("/receipts/{}/apply", fixture.receipt);
    let wrong = send(
        &fixture.router,
        Method::POST,
        &apply_path,
        Some(HOST),
        Some(ORIGIN),
        Some(&fixture.session.cookie),
        &format!("csrf={}&idempotency_key=wrong", fixture.session.csrf),
    )
    .await?;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let body = format!("csrf={}&idempotency_key={key}", fixture.session.csrf);
    let (first, replay) = tokio::join!(
        send(
            &fixture.router,
            Method::POST,
            &apply_path,
            Some(HOST),
            Some(ORIGIN),
            Some(&fixture.session.cookie),
            &body
        ),
        send(
            &fixture.router,
            Method::POST,
            &apply_path,
            Some(HOST),
            Some(ORIGIN),
            Some(&fixture.session.cookie),
            &body
        ),
    );
    let first = first?;
    let replay = replay?;
    assert_eq!(first.status, StatusCode::SEE_OTHER);
    assert_eq!(replay.status, StatusCode::SEE_OTHER);
    assert_eq!(first.headers.get(LOCATION), replay.headers.get(LOCATION));
    let job = first
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("apply job missing")?;
    wait_status(&fixture.router, &fixture.session, job, "Completed ·").await?;
    assert_eq!(mock.checks.load(Ordering::Relaxed), 1);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 1);
    assert_eq!(mock.reads.load(Ordering::Relaxed), 6);
    assert_eq!(
        fs::read_dir(fixture.workspace.path("state/checkpoints"))?.count(),
        1
    );
    let spent = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
    assert_eq!(spent.status, StatusCode::FORBIDDEN);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn expired_and_drifted_grants_never_construct_apply_clients()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    let expired = prepare_receipt(&origin, Some(1)).await?;
    let confirmed = confirm(&expired, &expired.digest, &expired.count, "").await?;
    let key = idempotency_key(&confirmed)?;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let response = apply(&expired.router, &expired.session, &expired.receipt, &key).await?;
    assert_eq!(response.status, StatusCode::GONE);
    assert_eq!(mock.reads.load(Ordering::Relaxed), 2);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 0);

    let drifted = prepare_receipt(&origin, None).await?;
    let confirmed = confirm(&drifted, &drifted.digest, &drifted.count, "").await?;
    let key = idempotency_key(&confirmed)?;
    fs::write(drifted.workspace.path("source/synthetic.jpg"), b"drifted\n")?;
    let response = apply(&drifted.router, &drifted.session, &drifted.receipt, &key).await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    let job = response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("drift refusal job missing")?;
    wait_status(&drifted.router, &drifted.session, job, "Failed ·").await?;
    assert_eq!(mock.reads.load(Ordering::Relaxed), 4);
    assert_eq!(mock.checks.load(Ordering::Relaxed), 0);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 0);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn logout_and_restart_forget_an_unused_grant() -> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    let fixture = prepare_receipt(&origin, None).await?;
    let confirmed = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
    let key = idempotency_key(&confirmed)?;
    let logout = send(
        &fixture.router,
        Method::POST,
        "/logout",
        Some(HOST),
        Some(ORIGIN),
        Some(&fixture.session.cookie),
        &format!("csrf={}", fixture.session.csrf),
    )
    .await?;
    assert_eq!(logout.status, StatusCode::SEE_OTHER);
    let logged_out = apply(&fixture.router, &fixture.session, &fixture.receipt, &key).await?;
    assert_eq!(logged_out.status, StatusCode::FORBIDDEN);

    let config = WebConfig::load(&fixture.workspace.path("web.toml"))?;
    let restarted = WebConsole::from_config(config)?.router();
    let restarted_session = pair(&restarted).await?;
    let forgotten = apply(&restarted, &restarted_session, &fixture.receipt, &key).await?;
    assert_eq!(forgotten.status, StatusCode::FORBIDDEN);
    assert_eq!(mock.reads.load(Ordering::Relaxed), 2);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 0);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_apply_requires_a_fresh_dry_run_before_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    mock.reject_known.store(true, Ordering::Relaxed);
    mock.upload_response_delay_ms.store(500, Ordering::Relaxed);
    let fixture = prepare_receipt(&origin, None).await?;
    let confirmed = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
    let first = apply(
        &fixture.router,
        &fixture.session,
        &fixture.receipt,
        &idempotency_key(&confirmed)?,
    )
    .await?;
    let job = first
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("cancel target missing")?;
    for _attempt in 0..100 {
        if mock.uploads.load(Ordering::Relaxed) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 1);
    let cancelled = send(
        &fixture.router,
        Method::POST,
        &format!("{job}/cancel"),
        Some(HOST),
        Some(ORIGIN),
        Some(&fixture.session.cookie),
        &format!("csrf={}", fixture.session.csrf),
    )
    .await?;
    assert_eq!(cancelled.status, StatusCode::SEE_OTHER);
    wait_status(&fixture.router, &fixture.session, job, "Cancelled ·").await?;
    let stale = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
    assert_eq!(stale.status, StatusCode::FORBIDDEN);

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    mock.upload_response_delay_ms.store(0, Ordering::Relaxed);
    let dry_run = admit(
        &fixture.router,
        &fixture.session,
        &format!("/plans/{}/dry-run", fixture.plan),
    )
    .await?;
    let receipt = wait_for_link(
        &fixture.router,
        &fixture.session,
        &dry_run,
        "href=\"/receipts/",
    )
    .await?;
    let (digest, count) = receipt_fields(&fixture.router, &fixture.session, &receipt).await?;
    let confirmed = confirm_values(
        &fixture.router,
        &fixture.session,
        &receipt,
        &digest,
        &count,
        "",
    )
    .await?;
    let resumed = apply(
        &fixture.router,
        &fixture.session,
        &receipt,
        &idempotency_key(&confirmed)?,
    )
    .await?;
    let resumed_job = resumed
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("resume job missing")?;
    wait_status(
        &fixture.router,
        &fixture.session,
        resumed_job,
        "Completed ·",
    )
    .await?;
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 1);
    assert_eq!(mock.checks.load(Ordering::Relaxed), 2);
    for _attempt in 0..2 {
        let repeated = send(
            &fixture.router,
            Method::POST,
            &format!("{resumed_job}/cancel"),
            Some(HOST),
            Some(ORIGIN),
            Some(&fixture.session.cookie),
            &format!("csrf={}", fixture.session.csrf),
        )
        .await?;
        assert_eq!(repeated.status, StatusCode::SEE_OTHER);
    }
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn bounded_before_and_after_commit_faults_converge_once()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_fault(false).await?;
    exercise_fault(true).await
}

async fn exercise_fault(after_commit: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    if after_commit {
        mock.reject_known.store(true, Ordering::Relaxed);
        mock.upload_failures.store(1, Ordering::Relaxed);
    } else {
        mock.bulk_failures.store(1, Ordering::Relaxed);
    }
    let fixture = prepare_receipt(&origin, None).await?;
    let response = confirm_and_apply(&fixture).await?;
    let job = response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("fault job missing")?;
    wait_status(&fixture.router, &fixture.session, job, "Completed ·").await?;
    assert_eq!(mock.checks.load(Ordering::Relaxed), 2);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 1);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn server_profile_and_credential_binding_substitution_is_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    for (field, value) in [
        ("server_profile_sha256", json!("f".repeat(64))),
        ("credential_generation", json!(2)),
    ] {
        let fixture = prepare_receipt(&origin, None).await?;
        let confirmed = confirm(&fixture, &fixture.digest, &fixture.count, "").await?;
        let key = idempotency_key(&confirmed)?;
        let binding_path = fixture
            .workspace
            .path(&format!("state/plans/{}.binding.json", fixture.plan));
        let mut binding: serde_json::Value = serde_json::from_slice(&fs::read(&binding_path)?)?;
        binding[field] = value;
        fs::write(&binding_path, serde_json::to_vec_pretty(&binding)?)?;
        let response = apply(&fixture.router, &fixture.session, &fixture.receipt, &key).await?;
        assert_eq!(response.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(mock.reads.load(Ordering::Relaxed), 4);
    assert_eq!(mock.checks.load(Ordering::Relaxed), 0);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 0);
    server.stop().await?;
    Ok(())
}
