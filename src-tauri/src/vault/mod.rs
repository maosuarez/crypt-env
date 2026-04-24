use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::crypto::{self, CryptoKey};
use crate::db::{DbCategory, VaultDb};

// ─── Serializable types (shared with frontend) ────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct VaultItem {
    pub id: i64,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    pub categories: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub created: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Serialize)]
pub struct UnlockPayload {
    pub items: Vec<VaultItem>,
    pub categories: Vec<Category>,
}

// ─── Application state ────────────────────────────────────────────────────────

pub struct VaultState {
    pub db: VaultDb,
    /// Zeroizing garantiza que los 32 bytes se sobreescriben al asignar None o al drop.
    pub key: Option<Zeroizing<[u8; 32]>>,
}

impl VaultState {
    pub fn new(db: VaultDb) -> Self {
        VaultState { db, key: None }
    }
}

pub type SharedState = Arc<Mutex<VaultState>>;

// ─── Crypto helpers ───────────────────────────────────────────────────────────

fn decrypt_item(key: &CryptoKey, row_id: i64, data: &str) -> Result<VaultItem, String> {
    let json = crypto::decrypt(key, data)?;
    let mut item: VaultItem =
        serde_json::from_slice(&json).map_err(|e| format!("deserialize item: {e}"))?;
    item.id = row_id;
    Ok(item)
}

fn encrypt_item(key: &CryptoKey, item: &VaultItem) -> Result<String, String> {
    let json = serde_json::to_vec(item).map_err(|e| format!("serialize item: {e}"))?;
    crypto::encrypt(key, &json)
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn vault_is_setup(state: State<'_, SharedState>) -> Result<bool, String> {
    let s = state.lock().await;
    s.db.is_initialized().await
}

#[tauri::command]
pub async fn vault_unlock(
    password: String,
    state: State<'_, SharedState>,
) -> Result<UnlockPayload, String> {
    let mut s = state.lock().await;

    // Derive or create the encryption key
    let key = {
        let db = &s.db;
        if db.is_initialized().await? {
            let (salt, token) = db
                .get_meta()
                .await?
                .ok_or_else(|| "vault_meta row missing".to_string())?;
            crypto::unlock_vault_crypto(password.as_bytes(), &salt, &token)?
        } else {
            let (salt, token, k) = crypto::init_vault_crypto(password.as_bytes())?;
            db.init_vault(&salt, &token).await?;
            k
        }
    };

    s.key = Some(Zeroizing::new(key));

    // Decrypt all items
    let raw = s.db.list_items().await?;
    let items: Vec<VaultItem> = raw
        .into_iter()
        .filter_map(|(id, _, data, _)| decrypt_item(&key, id, &data).ok())
        .collect();

    // Load categories
    let cats = s
        .db
        .list_categories()
        .await?
        .into_iter()
        .map(|c| Category {
            id: c.cid,
            name: c.name,
            color: c.color,
        })
        .collect();

    Ok(UnlockPayload {
        items,
        categories: cats,
    })
}

#[tauri::command]
pub async fn vault_lock(state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().await;
    s.key = None;
    Ok(())
}

#[tauri::command]
pub async fn vault_get_items(state: State<'_, SharedState>) -> Result<Vec<VaultItem>, String> {
    let s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?;
    let raw = s.db.list_items().await?;
    Ok(raw
        .into_iter()
        .filter_map(|(id, _, data, _)| decrypt_item(key, id, &data).ok())
        .collect())
}

#[tauri::command]
pub async fn vault_save_item(
    item: VaultItem,
    state: State<'_, SharedState>,
) -> Result<VaultItem, String> {
    let s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?;
    let encrypted = encrypt_item(key, &item)?;
    let new_id = s
        .db
        .upsert_item(item.id, &item.item_type, &encrypted, &item.created)
        .await?;
    let mut saved = item;
    saved.id = new_id;
    Ok(saved)
}

#[tauri::command]
pub async fn vault_delete_item(id: i64, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.db.delete_item(id).await
}

#[tauri::command]
pub async fn vault_get_categories(
    state: State<'_, SharedState>,
) -> Result<Vec<Category>, String> {
    let s = state.lock().await;
    Ok(s.db
        .list_categories()
        .await?
        .into_iter()
        .map(|c| Category {
            id: c.cid,
            name: c.name,
            color: c.color,
        })
        .collect())
}

#[tauri::command]
pub async fn vault_save_categories(
    cats: Vec<Category>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let s = state.lock().await;
    let db_cats: Vec<DbCategory> = cats
        .into_iter()
        .map(|c| DbCategory {
            cid: c.id,
            name: c.name,
            color: c.color,
        })
        .collect();
    s.db.save_categories(&db_cats).await
}

#[tauri::command]
pub async fn vault_get_settings(
    state: State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
    let s = state.lock().await;
    let timeout = s
        .db
        .get_setting("auto_lock_timeout")
        .await?
        .unwrap_or_else(|| "5".into());
    let hotkey = s
        .db
        .get_setting("hotkey")
        .await?
        .unwrap_or_else(|| "Ctrl+Alt+Z".into());
    Ok(serde_json::json!({
        "autoLockTimeout": timeout.parse::<i64>().unwrap_or(5),
        "hotkey": hotkey
    }))
}

#[tauri::command]
pub async fn vault_change_password(
    current_password: String,
    new_password: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;

    // Verify current password against stored meta
    let (salt, token) = s
        .db
        .get_meta()
        .await?
        .ok_or("vault_meta missing")?;
    let old_key = crypto::unlock_vault_crypto(current_password.as_bytes(), &salt, &token)?;

    // Generate new crypto material
    let (new_salt, new_token, new_key) = crypto::init_vault_crypto(new_password.as_bytes())?;

    // Re-encrypt every item blob
    let raw = s.db.list_items().await?;
    let mut re_encrypted: Vec<(i64, String)> = Vec::with_capacity(raw.len());
    for (id, _, data, _) in &raw {
        let plaintext = crypto::decrypt(&old_key, data)?;
        let new_data = crypto::encrypt(&new_key, &plaintext)?;
        re_encrypted.push((*id, new_data));
    }

    // Atomic DB update: new salt/token + all re-encrypted items
    s.db.rekey(&new_salt, &new_token, re_encrypted).await?;

    // Update in-memory key
    s.key = Some(Zeroizing::new(new_key));

    Ok(())
}

#[tauri::command]
pub async fn vault_wipe(state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().await;
    s.key = None;
    s.db.wipe_and_reset().await
}

#[tauri::command]
pub async fn vault_save_settings(
    auto_lock_timeout: i64,
    hotkey: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let s = state.lock().await;
    s.db
        .set_setting("auto_lock_timeout", &auto_lock_timeout.to_string())
        .await?;
    s.db.set_setting("hotkey", &hotkey).await
}

#[tauri::command]
pub async fn vault_generate_mcp_token(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;

    // Generar token de 32 bytes hex
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();

    // Guardar en DB
    s.db.set_setting("mcp_token", &token).await?;

    // Escribir a archivo para el binario MCP
    let app_dir = app.path().app_data_dir()
        .map_err(|e| format!("path error: {e}"))?;
    let token_path = app_dir.join("mcp_token");
    std::fs::write(&token_path, &token)
        .map_err(|e| format!("write mcp token file: {e}"))?;

    Ok(token)
}

#[tauri::command]
pub async fn vault_get_mcp_token(
    state: State<'_, SharedState>,
) -> Result<Option<String>, String> {
    let s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.db.get_setting("mcp_token").await
}
