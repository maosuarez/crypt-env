// Integration tests for VaultDb using a real SQLite database in a temp directory.
// Run with: cargo test --test vault_integration

use crypt_env_lib::db::VaultDb;
use crypt_env_lib::project::{self, EnvironmentInput, EnvironmentVar, ProjectInput};
use crypt_env_lib::vault::{self, VaultItem};
use tempfile::tempdir;

/// Shared setup for the `inject_environment` read-guard tests below: a fresh
/// db, one project with its auto-created default environment, and one vault
/// item wired into that environment as `DB_PASSWORD`. Returns
/// `(db, vault_key, environment_id)`.
async fn setup_project_with_one_var(db: &VaultDb) -> ([u8; 32], i64) {
    let key: [u8; 32] = [0x11u8; 32];

    let project_id = project::save_project(db, ProjectInput {
        id: 0,
        name: "Demo".into(),
        description: None,
        template: "generic".into(),
        categories: vec![],
    }).await.unwrap();

    let projects = project::list_projects(db).await.unwrap();
    let default_env_id = projects
        .iter()
        .find(|p| p.id == project_id)
        .unwrap()
        .environments
        .iter()
        .find(|e| e.is_default)
        .unwrap()
        .id;

    let item = VaultItem {
        id: 0,
        item_type: "secret".into(),
        name: Some("DB_PASSWORD".into()),
        value: Some("hunter2".into()),
        url: None,
        username: None,
        password: None,
        title: None,
        description: None,
        command: None,
        shell: None,
        categories: None,
        notes: None,
        content: None,
        created: "2026-01-01T00:00:00Z".into(),
        is_global: None,
    };
    let item_id = vault::create_project_item(db, &key, &item, project_id).await.unwrap();

    project::save_environment(db, EnvironmentInput {
        id: default_env_id,
        project_id,
        name: "default".into(),
        is_default: true,
        paths: vec![],
        vars: vec![EnvironmentVar { id: 0, key: "DB_PASSWORD".into(), item_id }],
    }).await.unwrap();

    (key, default_env_id)
}

// Issue #3, plan §3.6: `inject_environment`'s read side must distinguish
// `ErrorKind::NotFound` (legitimate: create a new file) from every other
// error kind, which must abort the write rather than proceed as if the
// target were empty. This is the gate the WSL bridge plan declared but
// deferred to #8 — landed here as the minimal `match e.kind()` guard.
#[tokio::test]
async fn test_inject_environment_aborts_on_non_not_found_read_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();
    let (key, env_id) = setup_project_with_one_var(&db).await;

    // A directory, not a file: std::fs::read_to_string fails with a
    // non-NotFound error kind. Before the guard, `unwrap_or_default()`
    // swallowed this and silently overwrote the "empty" content with only
    // this environment's keys — exactly the data-loss path the gate closes.
    let existing_dir = dir.path().join("not_a_file");
    std::fs::create_dir(&existing_dir).unwrap();
    let bad_path = existing_dir.to_str().unwrap().to_string();

    let result = project::inject_environment(&db, &key, env_id, Some(bad_path), None, false).await;
    assert!(
        result.is_err(),
        "a non-NotFound read error must abort the write, not be treated as an empty file"
    );
}

#[tokio::test]
async fn test_inject_environment_creates_new_file_on_not_found() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();
    let (key, env_id) = setup_project_with_one_var(&db).await;

    // Parent directory exists, target file does not — read must fail with
    // NotFound and be treated as "start from empty", not propagate an error.
    let target = dir.path().join(".env.new");
    let good_path = target.to_str().unwrap().to_string();

    let result = project::inject_environment(&db, &key, env_id, Some(good_path.clone()), None, false).await;
    assert!(result.is_ok(), "NotFound must still be treated as an empty starting file: {:?}", result.err());

    let content = std::fs::read_to_string(&good_path).unwrap();
    assert!(content.contains("DB_PASSWORD=hunter2"), "written content: {content}");
}

#[tokio::test]
async fn test_db_settings_roundtrip() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    db.set_setting("test_key", "hello").await.unwrap();
    let val = db.get_setting("test_key").await.unwrap();
    assert_eq!(val, Some("hello".to_string()));

    let missing = db.get_setting("nonexistent").await.unwrap();
    assert_eq!(missing, None);
}

#[tokio::test]
async fn test_db_settings_overwrite() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    db.set_setting("key", "first").await.unwrap();
    db.set_setting("key", "second").await.unwrap();
    let val = db.get_setting("key").await.unwrap();
    assert_eq!(val, Some("second".to_string()), "set_setting must overwrite existing value");
}

#[tokio::test]
async fn test_db_item_upsert_and_list() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let created = "2026-01-01T00:00:00Z";
    let encrypted_data = "deadbeef0102030405";

    let inserted_id = db.upsert_item(0, "env", encrypted_data, created, false).await.unwrap();
    assert!(inserted_id > 0, "inserted id must be positive");

    let items = db.list_items().await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].0, inserted_id);
    assert_eq!(items[0].1, "env");
    assert_eq!(items[0].2, encrypted_data);
    assert_eq!(items[0].3, created);
}

#[tokio::test]
async fn test_db_item_update() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let id = db.upsert_item(0, "env", "original_data", "2026-01-01", false).await.unwrap();
    db.upsert_item(id, "env", "updated_data", "2026-01-01", false).await.unwrap();

    let items = db.list_items().await.unwrap();
    assert_eq!(items.len(), 1, "update must not create a second row");
    assert_eq!(items[0].2, "updated_data");
}

#[tokio::test]
async fn test_db_item_delete() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let id = db.upsert_item(0, "env", "some_data", "2026-01-01", false).await.unwrap();
    db.delete_item(id).await.unwrap();

    let items = db.list_items().await.unwrap();
    assert!(items.is_empty(), "list must be empty after deleting the only item");
}

#[tokio::test]
async fn test_db_is_initialized_false_on_fresh_db() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let initialized = db.is_initialized().await.unwrap();
    assert!(!initialized, "fresh database must not be initialized");
}

#[tokio::test]
async fn test_db_init_vault_and_get_meta() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    db.init_vault("deadbeef_salt", "deadbeef_token").await.unwrap();

    let initialized = db.is_initialized().await.unwrap();
    assert!(initialized, "db must report initialized after init_vault");

    let meta = db.get_meta().await.unwrap();
    assert!(meta.is_some(), "get_meta must return Some after init_vault");
    let (salt, token) = meta.unwrap();
    assert_eq!(salt, "deadbeef_salt");
    assert_eq!(token, "deadbeef_token");
}
