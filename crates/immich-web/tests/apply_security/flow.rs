use std::fs;
use std::time::Duration;

use axum::Router;
use axum::http::header::LOCATION;
use axum::http::{Method, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};

use super::mock::SYNTHETIC_KEY;
use super::support::{
    HOST, ORIGIN, SECRET, TestResponse, TestWorkspace, csrf, response_cookie, send,
};

pub struct PairedSession {
    pub cookie: String,
    pub csrf: String,
}

pub struct ReceiptFixture {
    pub workspace: TestWorkspace,
    pub router: Router,
    pub session: PairedSession,
    pub plan: String,
    pub receipt: String,
    pub digest: String,
    pub count: String,
}

pub async fn pair(router: &Router) -> Result<PairedSession, Box<dyn std::error::Error>> {
    let page = send(router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    let pairing_cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let pairing_csrf = csrf(&page.body)?.to_owned();
    let paired = send(
        router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    let cookie = response_cookie(&paired.headers, "immich_rs_session")?;
    let dashboard = send(
        router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&cookie),
        "",
    )
    .await?;
    Ok(PairedSession {
        cookie,
        csrf: csrf(&dashboard.body)?.to_owned(),
    })
}

pub async fn admit(
    router: &Router,
    session: &PairedSession,
    path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = send(
        router,
        Method::POST,
        path,
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    if response.status != StatusCode::SEE_OTHER {
        return Err("job admission failed".into());
    }
    response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| "job location missing".into())
}

pub async fn wait_for_link(
    router: &Router,
    session: &PairedSession,
    job: &str,
    marker: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    for _attempt in 0..300 {
        let response = send(
            router,
            Method::GET,
            job,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        if let Some(value) = between(&response.body, marker, "\"") {
            return Ok(value.to_owned());
        }
        if response.body.contains("Failed ·") {
            return Err("background job failed".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("background job did not finish".into())
}

pub async fn wait_status(
    router: &Router,
    session: &PairedSession,
    job: &str,
    status: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for _attempt in 0..300 {
        let response = send(
            router,
            Method::GET,
            job,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        if response.body.contains(status) {
            return Ok(());
        }
        if response.body.contains("Failed ·") && status != "Failed ·" {
            return Err("background job failed".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("background job did not reach expected status".into())
}

pub async fn prepare_receipt(
    origin: &str,
    grant_seconds: Option<u64>,
) -> Result<ReceiptFixture, Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    drop(workspace.console()?);
    fs::write(
        workspace.path("never-read-api-key.secret"),
        format!("{SYNTHETIC_KEY}\n"),
    )?;
    make_private(&workspace.path("never-read-api-key.secret"))?;
    let console = workspace.console_with_server(origin)?;
    let console = if let Some(seconds) = grant_seconds {
        drop(console);
        let config_path = workspace.path("web.toml");
        let config = fs::read_to_string(&config_path)?.replace(
            "header_read_seconds = 1",
            &format!("header_read_seconds = 1\nproduction_grant_seconds = {seconds}"),
        );
        fs::write(&config_path, config)?;
        WebConsole::from_config(WebConfig::load(&config_path)?)?
    } else {
        console
    };
    let router = console.router();
    let session = pair(&router).await?;
    let plan_job = admit(
        &router,
        &session,
        "/sources/camera_roll/servers/disposable/plan",
    )
    .await?;
    let plan = wait_for_link(&router, &session, &plan_job, "href=\"/plans/").await?;
    let dry_run_job = admit(&router, &session, &format!("/plans/{plan}/dry-run")).await?;
    let receipt = wait_for_link(&router, &session, &dry_run_job, "href=\"/receipts/").await?;
    let (digest, count) = receipt_fields(&router, &session, &receipt).await?;
    Ok(ReceiptFixture {
        workspace,
        router,
        session,
        plan,
        receipt,
        digest,
        count,
    })
}

pub async fn receipt_fields(
    router: &Router,
    session: &PairedSession,
    receipt: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let page = send(
        router,
        Method::GET,
        &format!("/receipts/{receipt}"),
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    let digest = between(&page.body, "<dt>Plan digest</dt><dd><code>", "</code>")
        .ok_or("plan digest missing")?
        .to_owned();
    let count = between(&page.body, "<dt>Maximum logical effects</dt><dd>", "</dd>")
        .ok_or("logical-effect count missing")?
        .to_owned();
    Ok((digest, count))
}

pub async fn confirm(
    fixture: &ReceiptFixture,
    digest: &str,
    count: &str,
    backup: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    confirm_values(
        &fixture.router,
        &fixture.session,
        &fixture.receipt,
        digest,
        count,
        backup,
    )
    .await
}

pub async fn confirm_values(
    router: &Router,
    session: &PairedSession,
    receipt: &str,
    digest: &str,
    count: &str,
    backup: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(
        router,
        Method::POST,
        &format!("/receipts/{receipt}/confirm"),
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!(
            "csrf={}&plan_sha256={digest}&max_logical_effects={count}&backup_reference={backup}&acknowledge=apply",
            session.csrf
        ),
    )
    .await
}

pub async fn apply(
    router: &Router,
    session: &PairedSession,
    receipt: &str,
    key: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(
        router,
        Method::POST,
        &format!("/receipts/{receipt}/apply"),
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}&idempotency_key={key}", session.csrf),
    )
    .await
}

pub async fn confirm_and_apply(
    fixture: &ReceiptFixture,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    let confirmed = confirm(fixture, &fixture.digest, &fixture.count, "").await?;
    let key = idempotency_key(&confirmed)?;
    apply(&fixture.router, &fixture.session, &fixture.receipt, &key).await
}

pub fn idempotency_key(response: &TestResponse) -> Result<String, Box<dyn std::error::Error>> {
    between(&response.body, "name=\"idempotency_key\" value=\"", "\"")
        .map(str::to_owned)
        .ok_or_else(|| "idempotency key missing".into())
}

fn between<'a>(body: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let value = body.get(body.find(start)?.saturating_add(start.len())..)?;
    value.get(..value.find(end)?)
}

#[cfg(unix)]
fn make_private(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
