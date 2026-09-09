#![forbid(unsafe_code)]

use std::fs;
use std::sync::atomic::Ordering;

use axum::Router;
use axum::http::{Method, StatusCode};

#[path = "source_import_security/support.rs"]
mod import_support;
#[path = "apply_security/mock.rs"]
mod mock;
#[allow(dead_code)]
mod support;

use import_support::{
    admit, between, configured_console, create_sources, definition, link_reference, pair,
    wait_terminal,
};
use mock::start_mock;
use support::{HOST, ORIGIN, TestWorkspace, send};

struct Session {
    cookie: String,
    csrf: String,
}

struct Planned {
    reference: String,
    digest: String,
    effects: u64,
}

#[tokio::test]
async fn typed_imports_scan_plan_and_dry_run_without_browser_paths_or_offline_clients()
-> Result<(), Box<dyn std::error::Error>> {
    let (origin, mock_state, server) = start_mock().await?;
    let workspace = TestWorkspace::new()?;
    create_sources(&workspace)?;
    let router = configured_console(&workspace, &origin)?.router();
    let session = pair(&router).await?;

    verify_source_scans(&router, &session, &workspace).await?;
    let plans = plan_sources(&router, &session, &origin).await?;
    assert_eq!(mock_state.reads.load(Ordering::Relaxed), 6);
    server.stop().await?;
    fs::remove_file(workspace.path("api-key.secret"))?;
    verify_offline_dry_runs(&router, &session, &plans).await?;
    assert_eq!(mock_state.reads.load(Ordering::Relaxed), 6);
    assert_eq!(
        fs::read_dir(workspace.path("state/checkpoints"))?.count(),
        0
    );
    verify_import_identity_drift(&router, &session, &workspace, &plans).await?;
    assert_eq!(mock_state.reads.load(Ordering::Relaxed), 6);
    Ok(())
}

async fn verify_source_scans(
    router: &Router,
    session: &Session,
    workspace: &TestWorkspace,
) -> Result<(), Box<dyn std::error::Error>> {
    let dashboard = send(
        router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    for label in ["Google Takeout", "Apple Photos", "Picasa"] {
        assert!(dashboard.body.contains(label));
    }
    assert!(
        !dashboard
            .body
            .contains(&workspace.path("").display().to_string())
    );

    for (source, schema) in [("takeout", 2), ("apple", 3), ("picasa", 4)] {
        let job = admit(router, session, &format!("/sources/{source}/scan")).await?;
        let body = wait_terminal(router, session, &job, source, false).await?;
        assert!(body.contains("Completed ·"));
        assert!(body.contains(&format!("<dd>{schema}</dd>")));
        assert!(!body.contains("/plans/"));
    }
    Ok(())
}

async fn plan_sources(
    router: &Router,
    session: &Session,
    origin: &str,
) -> Result<Vec<Planned>, Box<dyn std::error::Error>> {
    let mut plans = Vec::new();
    for source in ["takeout", "apple", "picasa"] {
        let job = admit(
            router,
            session,
            &format!("/sources/{source}/servers/disposable/plan"),
        )
        .await?;
        let body = wait_terminal(router, session, &job, source, false).await?;
        let reference = link_reference(&body, "href=\"/plans/")
            .ok_or("source-import plan reference missing")?;
        let inspection = send(
            router,
            Method::GET,
            &format!("/plans/{reference}"),
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        assert_eq!(inspection.status, StatusCode::OK);
        assert!(!inspection.body.contains(origin));
        plans.push(Planned {
            reference,
            digest: between(
                &inspection.body,
                "<h2>Canonical plan digest</h2>\n  <p><code>",
            )?,
            effects: definition(&inspection.body, "Maximum logical effects")?.parse()?,
        });
    }
    Ok(plans)
}

async fn verify_offline_dry_runs(
    router: &Router,
    session: &Session,
    plans: &[Planned],
) -> Result<(), Box<dyn std::error::Error>> {
    for plan in plans {
        let job = admit(
            router,
            session,
            &format!("/plans/{}/dry-run", plan.reference),
        )
        .await?;
        let body = wait_terminal(router, session, &job, &plan.reference, false).await?;
        let receipt =
            link_reference(&body, "href=\"/receipts/").ok_or("source-import receipt missing")?;
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
        assert!(page.body.contains("Apply unavailable"));
        let confirm = send(
            router,
            Method::POST,
            &format!("/receipts/{receipt}/confirm"),
            Some(HOST),
            Some(ORIGIN),
            Some(&session.cookie),
            &format!(
                "csrf={}&plan_sha256={}&max_logical_effects={}&backup_reference=&acknowledge=apply",
                session.csrf, plan.digest, plan.effects
            ),
        )
        .await?;
        assert_eq!(confirm.status, StatusCode::FORBIDDEN);
    }
    Ok(())
}

async fn verify_import_identity_drift(
    router: &Router,
    session: &Session,
    workspace: &TestWorkspace,
    plans: &[Planned],
) -> Result<(), Box<dyn std::error::Error>> {
    fs::rename(
        workspace.path("sources/apple"),
        workspace.path("sources/parked"),
    )?;
    fs::create_dir(workspace.path("sources/apple"))?;
    let apple_plan = plans.get(1).ok_or("Apple plan missing")?;
    let drifted = admit(
        router,
        session,
        &format!("/plans/{}/dry-run", apple_plan.reference),
    )
    .await?;
    let _failed = wait_terminal(router, session, &drifted, "drifted Apple", true).await?;
    Ok(())
}
