#![forbid(unsafe_code)]

use std::fs;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::http::header::LOCATION;
use axum::http::{Method, StatusCode};

#[path = "source_import_security/support.rs"]
#[allow(dead_code)]
mod import_support;
#[path = "apply_security/mock.rs"]
mod mock;
#[allow(dead_code)]
mod support;

use import_support::{
    admit, configured_console, create_sources, link_reference, pair, wait_terminal,
};
use mock::start_mock;
use support::{HOST, ORIGIN, TestResponse, TestWorkspace, send};

struct Session {
    cookie: String,
    csrf: String,
}

struct ImportReceipt {
    plan: String,
    receipt: String,
    digest: String,
    effects: String,
}

#[tokio::test]
async fn all_supported_imports_apply_once_and_fresh_plans_converge()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    let workspace = TestWorkspace::new()?;
    create_sources(&workspace)?;
    let router = configured_console(&workspace, &origin)?.router();
    let session = pair(&router).await?;

    run_import_set(&router, &session, false).await?;
    assert_eq!(mock.checks.load(Ordering::Relaxed), 3);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 3);
    assert_eq!(mock.metadata_updates.load(Ordering::Relaxed), 1);
    assert_eq!(mock.album_creates.load(Ordering::Relaxed), 1);
    assert_eq!(mock.album_memberships.load(Ordering::Relaxed), 2);

    mock.reject_known.store(true, Ordering::Relaxed);
    run_import_set(&router, &session, true).await?;
    assert_eq!(mock.checks.load(Ordering::Relaxed), 6);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 3);
    assert_eq!(mock.metadata_updates.load(Ordering::Relaxed), 2);
    assert_eq!(mock.album_creates.load(Ordering::Relaxed), 1);
    assert_eq!(mock.album_memberships.load(Ordering::Relaxed), 4);
    assert_eq!(
        fs::read_dir(workspace.path("state/checkpoints"))?.count(),
        6
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn import_source_drift_after_confirmation_fails_before_apply_client()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    let workspace = TestWorkspace::new()?;
    create_sources(&workspace)?;
    let router = configured_console(&workspace, &origin)?.router();
    let session = pair(&router).await?;
    let receipt = prepare_import(&router, &session, "apple").await?;
    let confirmed = confirm_receipt(&router, &session, &receipt).await?;
    let key = between(&confirmed.body, "name=\"idempotency_key\" value=\"", "\"")?;
    fs::write(
        workspace.path("sources/apple/Synthetic Album/apple.jpg"),
        b"changed synthetic apple\n",
    )?;
    let admitted = apply(&router, &session, &receipt.receipt, &key).await?;
    assert_eq!(admitted.status, StatusCode::SEE_OTHER);
    let job = admitted
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("drifted Apple apply job missing")?;
    let _failed = wait_terminal(&router, &session, job, "drifted Apple apply", true).await?;
    assert_eq!(mock.reads.load(Ordering::Relaxed), 2);
    assert_eq!(mock.checks.load(Ordering::Relaxed), 0);
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 0);
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_import_requires_fresh_dry_run_then_resumes_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock, server) = start_mock().await?;
    mock.reject_known.store(true, Ordering::Relaxed);
    mock.upload_response_delay_ms.store(500, Ordering::Relaxed);
    let workspace = TestWorkspace::new()?;
    create_sources(&workspace)?;
    let router = configured_console(&workspace, &origin)?.router();
    let session = pair(&router).await?;
    let first_receipt = prepare_import(&router, &session, "takeout").await?;
    let confirmed = confirm_receipt(&router, &session, &first_receipt).await?;
    let key = between(&confirmed.body, "name=\"idempotency_key\" value=\"", "\"")?;
    let first = apply(&router, &session, &first_receipt.receipt, &key).await?;
    let first_job = location(&first)?;
    for _attempt in 0..100 {
        if mock.uploads.load(Ordering::Relaxed) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let cancelled = send(
        &router,
        Method::POST,
        &format!("{first_job}/cancel"),
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(cancelled.status, StatusCode::SEE_OTHER);
    wait_status(&router, &session, first_job, "Cancelled ·").await?;
    let stale = confirm_receipt(&router, &session, &first_receipt).await?;
    assert_eq!(stale.status, StatusCode::FORBIDDEN);

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    mock.upload_response_delay_ms.store(0, Ordering::Relaxed);
    let resumed_receipt = dry_run_plan(&router, &session, &first_receipt.plan, "resume").await?;
    let confirmed = confirm_receipt(&router, &session, &resumed_receipt).await?;
    let key = between(&confirmed.body, "name=\"idempotency_key\" value=\"", "\"")?;
    let resumed = apply(&router, &session, &resumed_receipt.receipt, &key).await?;
    let resumed_job = location(&resumed)?;
    let _completed = wait_terminal(&router, &session, resumed_job, "resumed import", false).await?;
    assert_eq!(mock.uploads.load(Ordering::Relaxed), 1);
    assert_eq!(mock.checks.load(Ordering::Relaxed), 2);
    assert_eq!(
        fs::read_dir(workspace.path("state/checkpoints"))?.count(),
        1
    );
    server.stop().await?;
    Ok(())
}

async fn run_import_set(
    router: &Router,
    session: &Session,
    replay_admission: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    for source in ["takeout", "apple", "picasa"] {
        let receipt = prepare_import(router, session, source).await?;
        let confirmed = confirm_receipt(router, session, &receipt).await?;
        assert_eq!(confirmed.status, StatusCode::OK);
        let key = between(&confirmed.body, "name=\"idempotency_key\" value=\"", "\"")?;
        let body = format!("csrf={}&idempotency_key={key}", session.csrf);
        let first = apply_body(router, session, &receipt.receipt, &body).await?;
        assert_eq!(first.status, StatusCode::SEE_OTHER);
        if replay_admission {
            let replay = send(
                router,
                Method::POST,
                &format!("/receipts/{}/apply", receipt.receipt),
                Some(HOST),
                Some(ORIGIN),
                Some(&session.cookie),
                &body,
            )
            .await?;
            assert_eq!(first.headers.get(LOCATION), replay.headers.get(LOCATION));
        }
        let job = location(&first)?;
        let _completed = wait_terminal(router, session, job, source, false).await?;
    }
    Ok(())
}

async fn prepare_import(
    router: &Router,
    session: &Session,
    source: &str,
) -> Result<ImportReceipt, Box<dyn std::error::Error>> {
    let plan_job = admit(
        router,
        session,
        &format!("/sources/{source}/servers/disposable/plan"),
    )
    .await?;
    let plan_page = wait_terminal(router, session, &plan_job, source, false).await?;
    let plan =
        link_reference(&plan_page, "href=\"/plans/").ok_or("source-import apply plan missing")?;
    dry_run_plan(router, session, &plan, source).await
}

async fn dry_run_plan(
    router: &Router,
    session: &Session,
    plan: &str,
    context: &str,
) -> Result<ImportReceipt, Box<dyn std::error::Error>> {
    let dry_job = admit(router, session, &format!("/plans/{plan}/dry-run")).await?;
    let dry_page = wait_terminal(router, session, &dry_job, context, false).await?;
    let receipt = link_reference(&dry_page, "href=\"/receipts/")
        .ok_or("source-import apply receipt missing")?;
    let page = get(router, session, &format!("/receipts/{receipt}")).await?;
    Ok(ImportReceipt {
        plan: plan.to_owned(),
        receipt,
        digest: between(&page.body, "<dt>Plan digest</dt><dd><code>", "</code>")?,
        effects: between(&page.body, "<dt>Maximum logical effects</dt><dd>", "</dd>")?,
    })
}

async fn confirm_receipt(
    router: &Router,
    session: &Session,
    receipt: &ImportReceipt,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    confirm(
        router,
        session,
        &receipt.receipt,
        &receipt.digest,
        &receipt.effects,
    )
    .await
}

async fn confirm(
    router: &Router,
    session: &Session,
    receipt: &str,
    digest: &str,
    effects: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(
        router,
        Method::POST,
        &format!("/receipts/{receipt}/confirm"),
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!(
            "csrf={}&plan_sha256={digest}&max_logical_effects={effects}&backup_reference=&acknowledge=apply",
            session.csrf
        ),
    )
    .await
}

async fn apply(
    router: &Router,
    session: &Session,
    receipt: &str,
    key: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    let body = format!("csrf={}&idempotency_key={key}", session.csrf);
    apply_body(router, session, receipt, &body).await
}

async fn apply_body(
    router: &Router,
    session: &Session,
    receipt: &str,
    body: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(
        router,
        Method::POST,
        &format!("/receipts/{receipt}/apply"),
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        body,
    )
    .await
}

async fn get(
    router: &Router,
    session: &Session,
    path: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(
        router,
        Method::GET,
        path,
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await
}

fn between(body: &str, start: &str, end: &str) -> Result<String, Box<dyn std::error::Error>> {
    let remaining = body
        .get(body.find(start).ok_or("field missing")? + start.len()..)
        .ok_or("field invalid")?;
    Ok(remaining
        .get(..remaining.find(end).ok_or("field terminator missing")?)
        .ok_or("field value missing")?
        .to_owned())
}

fn location(response: &TestResponse) -> Result<&str, Box<dyn std::error::Error>> {
    response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "source-import apply job missing".into())
}

async fn wait_status(
    router: &Router,
    session: &Session,
    job: &str,
    status: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for _attempt in 0..300 {
        let page = get(router, session, job).await?;
        if page.body.contains(status) {
            return Ok(());
        }
        if page.body.contains("Failed ·") || page.body.contains("Completed ·") {
            return Err("source-import apply reached an unexpected terminal status".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("source-import apply did not reach expected status".into())
}
