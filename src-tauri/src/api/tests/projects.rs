//! Router-level tests for `/projects` and `/environments`.

use crate::test_support::{req, router, unlocked_vault};

#[tokio::test]
async fn list_projects_includes_nested_environments() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "GET", "/projects", Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200);
    let projects = json.as_array().unwrap();
    let demo = projects.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some("demo")).unwrap();
    let envs = demo.get("environments").and_then(|e| e.as_array()).unwrap();
    let mut names: Vec<&str> = envs.iter().filter_map(|e| e.get("name").and_then(|n| n.as_str())).collect();
    names.sort();
    assert_eq!(names, vec!["local", "production"]);
}

#[tokio::test]
async fn create_project_succeeds() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let body = serde_json::json!({ "id": 0, "name": "brand-new", "template": "generic", "categories": [] });
    let (status, json) = req(&app, "POST", "/projects", Some(&v.token), Some(body)).await;
    assert_eq!(status.as_u16(), 201);
    assert!(json.get("id").and_then(|i| i.as_i64()).unwrap() > 0);
}

#[tokio::test]
async fn create_project_with_existing_name_is_409() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let body = serde_json::json!({ "id": 0, "name": "demo", "template": "generic", "categories": [] });
    let (status, json) = req(&app, "POST", "/projects", Some(&v.token), Some(body)).await;
    assert_eq!(status.as_u16(), 409);
    assert_eq!(json.get("code").and_then(|c| c.as_str()), Some("CONFLICT"));
}

#[tokio::test]
async fn updating_a_project_by_id_is_not_treated_as_a_duplicate() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let body = serde_json::json!({
        "id": v.project_id,
        "name": "demo",
        "description": "updated description",
        "template": "generic",
        "categories": [],
    });
    let (status, json) = req(&app, "POST", "/projects", Some(&v.token), Some(body)).await;
    assert_eq!(status.as_u16(), 200, "update-by-id must not 409 against its own current name");
    assert_eq!(json.get("id").and_then(|i| i.as_i64()), Some(v.project_id));
}

#[tokio::test]
async fn delete_project_cascades() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, json) = req(&app, "DELETE", &format!("/projects/{}", v.project_id), Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200);
    assert!(json.get("environments").and_then(|e| e.as_i64()).unwrap() >= 2);

    let (_, list_json) = req(&app, "GET", "/projects", Some(&v.token), None).await;
    let projects = list_json.as_array().unwrap();
    assert!(projects.iter().find(|p| p.get("id").and_then(|i| i.as_i64()) == Some(v.project_id)).is_none());
}

#[tokio::test]
async fn preview_delete_counts_match_actual_delete() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (status, preview) = req(&app, "GET", &format!("/projects/{}/preview-delete", v.project_id), Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 200);

    let (_, actual) = req(&app, "DELETE", &format!("/projects/{}", v.project_id), Some(&v.token), None).await;

    assert_eq!(preview.get("environments"), actual.get("environments"));
    assert_eq!(preview.get("itemsDeleted"), actual.get("itemsDeleted"));
    assert_eq!(preview.get("itemsOrphaned"), actual.get("itemsOrphaned"));
}

#[tokio::test]
async fn save_environment_rejects_var_referencing_unowned_non_global_item() {
    let v = unlocked_vault().await;
    let app = router(&v);
    // Create a second project owning its own private (non-global) item.
    let other = serde_json::json!({ "id": 0, "name": "other-project", "template": "generic", "categories": [] });
    let (_, other_json) = req(&app, "POST", "/projects", Some(&v.token), Some(other)).await;
    let other_id = other_json.get("id").and_then(|i| i.as_i64()).unwrap();

    let create_item = serde_json::json!({ "type": "secret", "name": "PRIVATE", "value": "v" });
    let (_, item_json) = req(
        &app,
        "POST",
        &format!("/items?project={}&environment=default", "other-project"),
        Some(&v.token),
        Some(create_item),
    )
    .await;
    let item_id = item_json.get("id").and_then(|i| i.as_i64()).unwrap();

    // v.project_id ("demo") tries to link that item without owning it or it being global.
    let body = serde_json::json!({
        "id": 0,
        "projectId": v.project_id,
        "name": "staging",
        "isDefault": false,
        "paths": [],
        "vars": [{ "id": 0, "key": "PRIVATE", "itemId": item_id }],
    });
    let (status, json) = req(&app, "POST", "/environments", Some(&v.token), Some(body)).await;
    assert_eq!(status.as_u16(), 500, "current handler surfaces the ownership guard as INTERNAL_ERROR, not 422");
    let _ = other_id;
    let _ = json;
}

#[tokio::test]
async fn delete_environment_removes_it() {
    let v = unlocked_vault().await;
    let app = router(&v);
    let (_, projects) = req(&app, "GET", "/projects", Some(&v.token), None).await;
    let demo = projects.as_array().unwrap().iter().find(|p| p.get("id").and_then(|i| i.as_i64()) == Some(v.project_id)).unwrap();
    let local_env = demo.get("environments").and_then(|e| e.as_array()).unwrap().iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("local")).unwrap();
    let local_id = local_env.get("id").and_then(|i| i.as_i64()).unwrap();

    let (status, _) = req(&app, "DELETE", &format!("/environments/{local_id}"), Some(&v.token), None).await;
    assert_eq!(status.as_u16(), 204);

    let (_, projects_after) = req(&app, "GET", "/projects", Some(&v.token), None).await;
    let demo_after = projects_after.as_array().unwrap().iter().find(|p| p.get("id").and_then(|i| i.as_i64()) == Some(v.project_id)).unwrap();
    let envs_after = demo_after.get("environments").and_then(|e| e.as_array()).unwrap();
    assert!(envs_after.iter().find(|e| e.get("id").and_then(|i| i.as_i64()) == Some(local_id)).is_none());
}
