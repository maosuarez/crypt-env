// Integration tests for VaultDb using a real SQLite database in a temp directory.
// Run with: cargo test --test vault_integration

use crypt_env_lib::db::{LinkMode, VaultDb};
use crypt_env_lib::vault;
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

// ─── Issue #9: db-layer tests (12-15 of the plan's test matrix) ───────────
//
// Tests 1-11 need an HTTP-level harness (bound router + token + unlocked
// temp vault) that issue #11 owns and had not landed in this worktree at the
// time of writing — see the plan's §3.9 and this repo's parallel-worktree
// setup. Consuming that harness for the `api`-level create-or-update-on-
// collision assertions (200/201/409 status codes, `SHARED_ITEM_CONFLICT` /
// `KEY_EXISTS` / `CONFLICT_RETRY` bodies) is a documented follow-up once
// `crate::test_support` / `TestVault` land here. These four are pure
// `VaultDb` tests and need no HTTP surface at all.

#[tokio::test]
async fn test_link_item_rolls_back_when_link_fails() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let project_id = db.upsert_project(0, "acme", None, "generic").await.unwrap();
    let bogus_environment_id = 999_999; // no such environment exists

    let result = db
        .create_or_link_item(
            bogus_environment_id,
            project_id,
            "API_KEY",
            "secret",
            "ciphertext-blob",
            "2026-01-01T00:00:00Z",
            LinkMode::Update,
            None, // caller's inspect_env_key saw a free key
        )
        .await;

    assert!(
        result.is_err(),
        "linking into a nonexistent environment must fail (FK violation on environment_vars)"
    );

    let items = db.list_items().await.unwrap();
    assert!(items.is_empty(), "the item insert must be rolled back with the rest of the transaction");

    let owned = db.list_owned_item_ids(project_id).await.unwrap();
    assert!(owned.is_empty(), "the item_projects insert must be rolled back too — no orphaned ownership row");
}

#[tokio::test]
async fn test_list_orphan_items_finds_unlinked_nonglobal() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let id = db.upsert_item(0, "secret", "ciphertext", "2026-01-01", false).await.unwrap();
    // No environment_vars row created — this item is unreferenced.

    let orphans = db.list_orphan_item_ids().await.unwrap();
    assert_eq!(orphans, vec![id], "an unlinked, non-global item must be reported as an orphan");
}

#[tokio::test]
async fn test_list_orphan_items_excludes_global_and_linked() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    // Unlinked but global: has a reachable surface (Global Secrets) even
    // without a link, so it must NOT be reported as an orphan.
    let global_id = db.upsert_item(0, "secret", "global-ciphertext", "2026-01-01", true).await.unwrap();

    // Non-global but linked: reachable via its environment's scoped list,
    // so it must NOT be reported as an orphan either.
    let linked_id = db.upsert_item(0, "secret", "linked-ciphertext", "2026-01-01", false).await.unwrap();
    let project_id = db.upsert_project(0, "acme", None, "generic").await.unwrap();
    let environment_id = db.upsert_environment(0, project_id, "production", true).await.unwrap();
    db.upsert_environment_var(environment_id, "DB_PASSWORD", linked_id).await.unwrap();

    let orphans = db.list_orphan_item_ids().await.unwrap();
    assert!(!orphans.contains(&global_id), "a global unlinked item must not be reported as an orphan");
    assert!(!orphans.contains(&linked_id), "a linked non-global item must not be reported as an orphan");
    assert!(orphans.is_empty(), "no other rows were created in this test");
}

#[tokio::test]
async fn test_prune_orphan_items_clears_item_projects() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = VaultDb::open(&db_path).await.unwrap();

    let project_id = db.upsert_project(0, "acme", None, "generic").await.unwrap();

    // The orphan to be pruned: owned, but never linked into any environment.
    let orphan_id = db.upsert_item(0, "secret", "orphan-ciphertext", "2026-01-01", false).await.unwrap();
    db.add_item_owner(orphan_id, project_id).await.unwrap();

    // A second, unrelated item that must survive the prune untouched.
    let kept_id = db.upsert_item(0, "secret", "kept-ciphertext", "2026-01-01", false).await.unwrap();
    db.add_item_owner(kept_id, project_id).await.unwrap();
    let environment_id = db.upsert_environment(0, project_id, "production", true).await.unwrap();
    db.upsert_environment_var(environment_id, "DB_PASSWORD", kept_id).await.unwrap();

    vault::prune_orphan_items(&db, &[orphan_id]).await.unwrap();

    let items = db.list_items().await.unwrap();
    assert!(
        items.iter().all(|(id, ..)| *id != orphan_id),
        "the orphan's items row must be deleted"
    );
    assert!(
        db.list_owned_item_ids(project_id).await.unwrap().iter().all(|id| *id != orphan_id),
        "the orphan's item_projects row must be deleted too"
    );

    assert!(items.iter().any(|(id, ..)| *id == kept_id), "an item not in the prune list must survive");
    assert!(
        db.list_owned_item_ids(project_id).await.unwrap().contains(&kept_id),
        "the surviving item's ownership link must be untouched"
    );
}
