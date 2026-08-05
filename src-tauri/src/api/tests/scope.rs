//! Router-level tests for project/environment scope resolution (R1 in the
//! plan). Mirrors `project::resolve_environment`'s own in-file suite but
//! through the real HTTP surface (`GET /items`), including auth precedence.

use crate::test_support::{link_var, locked_vault, req, router, seed_item, seed_project, unlocked_vault, unlocked_vault_empty};

#[tokio::test]
async fn resolves_by_environment_id() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", &format!("/items?environment_id={}&include_global=false", v.env_id), Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200u16);
    assert_eq!(json.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn resolves_by_project_and_environment_names() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", "/items?project=demo&environment=production&include_global=false", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200u16);
    assert_eq!(json.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn project_name_is_case_insensitive() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, _) = req(&app, "GET", "/items?project=DEMO&environment=production", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200u16);
}

#[tokio::test]
async fn environment_name_is_case_insensitive() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, _) = req(&app, "GET", "/items?project=demo&environment=PRODUCTION", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200u16);
}

#[tokio::test]
async fn project_alone_without_environment_is_422() {
    // Current behaviour: `resolve_environment` has no "project alone ->
    // default environment" fallback (see project::mod's own test of the
    // same name). At the HTTP layer that surfaces as 422 VALIDATION_ERROR.
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", "/items?project=demo", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16);
    assert_eq!(json.get("code").and_then(|c| c.as_str()), Some("VALIDATION_ERROR"));
}

#[tokio::test]
async fn no_scope_params_at_all_is_422() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", "/items", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16);
    assert_eq!(json.get("code").and_then(|c| c.as_str()), Some("VALIDATION_ERROR"));
}

#[tokio::test]
async fn unknown_environment_id_is_422() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, _) = req(&app, "GET", "/items?environment_id=999999", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16);
}

#[tokio::test]
async fn unknown_project_name_is_422() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, _) = req(&app, "GET", "/items?project=ghost&environment=production", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16);
}

#[tokio::test]
async fn known_project_unknown_environment_is_422() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, _) = req(&app, "GET", "/items?project=demo&environment=ghost", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16);
}

#[tokio::test]
async fn environment_id_takes_precedence_over_project_and_environment() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let uri = format!(
        "/items?environment_id={}&project=does-not-exist&environment=also-not-real",
        v.env_id
    );
    let (status, _) = req(&app, "GET", &uri, Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200u16, "environment_id must win even with a nonsense project/environment pair");
}

#[tokio::test]
async fn missing_token_is_401() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", &format!("/items?environment_id={}", v.env_id), None, None).await;
    assert_eq!(status.as_u16(), 401u16);
    assert_eq!(json.get("code").and_then(|c| c.as_str()), Some("UNAUTHORIZED"));
}

#[tokio::test]
async fn locked_vault_with_valid_token_is_403_vault_locked() {
    let v = locked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", &format!("/items?environment_id={}", v.env_id), Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 403u16);
    assert_eq!(json.get("code").and_then(|c| c.as_str()), Some("VAULT_LOCKED"));
}

// Extra coverage beyond the 12 listed in the plan: an empty vault (no
// projects at all) must still 422 rather than panic, exercising the
// `unlocked_vault_empty` fixture and confirming `seed_project`/`seed_item`/
// `link_var` compose correctly for ad-hoc scopes.
#[tokio::test]
async fn empty_vault_can_be_scoped_via_seed_helpers() {
    let v = unlocked_vault_empty().await;
    let app = router(&v);

    let (status, _) = req(&app, "GET", "/items", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 422u16, "no projects exist yet");

    let (project_id, env_ids) = seed_project(&v, "adhoc", &["staging"]).await;
    let item_id = seed_item(&v, "TOKEN", "value", false).await;
    link_var(&v, env_ids[0], "TOKEN", item_id).await;

    let (status, json) = req(
        &app,
        "GET",
        &format!("/items?project=adhoc&environment=staging"),
        Some(&v.token),
        None,
    )
    .await;
    assert_eq!(status.as_u16(), 200u16);
    assert_eq!(json.as_array().unwrap().len(), 1);
    let _ = project_id;
}
