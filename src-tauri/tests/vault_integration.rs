// Integration tests for VaultDb using a real SQLite database in a temp directory.
// Run with: cargo test --test vault_integration

use crypt_env_lib::db::VaultDb;
use tempfile::tempdir;

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

// ─── Issue #13: global-item scoped visibility — data-path guards ─────────────
//
// The API's scope filter (src-tauri/src/api/mod.rs, `scope_items`) now reads
// `is_global` off every row returned by `list_items()` to decide whether an
// unlinked item should be unioned into a discovery response. These two tests
// guard the data path that decision depends on: the DB layer must round-trip
// `is_global` faithfully through both insert/update (`upsert_item`) and the
// dedicated toggle (`set_item_global`).

#[tokio::test]
async fn test_db_list_items_preserves_is_global_flag() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let global_id = db.upsert_item(0, "secret", "global_data", "2026-01-01", true).await.unwrap();
    let non_global_id = db.upsert_item(0, "secret", "scoped_data", "2026-01-01", false).await.unwrap();

    let items = db.list_items().await.unwrap();
    assert_eq!(items.len(), 2);

    let global_row = items.iter().find(|r| r.0 == global_id).expect("global item must be present");
    assert!(global_row.4, "tuple index 4 (is_global) must be true for the item created global");

    let scoped_row = items.iter().find(|r| r.0 == non_global_id).expect("non-global item must be present");
    assert!(!scoped_row.4, "tuple index 4 (is_global) must be false for the item created non-global");
}

#[tokio::test]
async fn test_db_set_item_global_roundtrip() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let id = db.upsert_item(0, "secret", "data", "2026-01-01", false).await.unwrap();

    db.set_item_global(id, true).await.unwrap();
    let items = db.list_items().await.unwrap();
    let row = items.iter().find(|r| r.0 == id).unwrap();
    assert!(row.4, "set_item_global(true) must flip is_global to true in list_items");

    db.set_item_global(id, false).await.unwrap();
    let items = db.list_items().await.unwrap();
    let row = items.iter().find(|r| r.0 == id).unwrap();
    assert!(!row.4, "set_item_global(false) must flip is_global back to false in list_items");
}
