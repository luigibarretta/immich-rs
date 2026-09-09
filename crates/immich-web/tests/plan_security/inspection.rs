use axum::Router;
use axum::http::header::CONTENT_DISPOSITION;
use axum::http::{Method, StatusCode};

use super::PairedSession;
use super::support::{HOST, TestWorkspace, send};

pub async fn inspect_and_export(
    router: &Router,
    session: &PairedSession,
    plan_ref: &str,
    workspace: &TestWorkspace,
    origin: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let locked = send(
        router,
        Method::GET,
        &format!("/plans/{plan_ref}"),
        Some(HOST),
        None,
        None,
        "",
    )
    .await?;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    let inspection = send(
        router,
        Method::GET,
        &format!("/plans/{plan_ref}"),
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(inspection.status, StatusCode::OK);
    assert!(inspection.body.contains("Canonical plan digest"));
    assert!(!inspection.body.contains(origin));
    assert!(
        !inspection
            .body
            .contains(&workspace.path("").display().to_string())
    );
    let export = send(
        router,
        Method::GET,
        &format!("/plans/{plan_ref}/export"),
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(export.status, StatusCode::OK);
    assert_eq!(
        export
            .headers
            .get(CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=immich-rs-plan.json")
    );
    let exported: immich_rs_application::UploadPlan = serde_json::from_str(&export.body)?;
    exported.validate()?;
    Ok(())
}
