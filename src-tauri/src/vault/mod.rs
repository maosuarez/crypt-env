use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Manager, State};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::biometric;
use crate::crypto::{self, CryptoKey};
use crate::db::{DbCategory, VaultDb};

pub mod import;
pub mod share_commands;

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
    /// Zeroizing guarantees the 32 bytes are overwritten when set to None or dropped.
    pub key: Option<Zeroizing<[u8; 32]>>,
    /// Monotonic timestamp of the last vault operation. None when the vault is locked.
    pub last_activity: Option<std::time::Instant>,
}

impl VaultState {
    pub fn new(db: VaultDb) -> Self {
        VaultState {
            db,
            key: None,
            last_activity: None,
        }
    }

    /// Record activity. Call this on every vault operation that accesses the key.
    pub fn touch(&mut self) {
        self.last_activity = Some(std::time::Instant::now());
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

/// Shared unlock logic: derives key from password bytes, loads items + categories.
/// The caller is responsible for verifying the password is correct before calling this
/// (either via `unlock_vault_crypto` or by knowing it came from a trusted DPAPI blob).
async fn do_unlock(
    password_bytes: &[u8],
    s: &mut VaultState,
) -> Result<UnlockPayload, String> {
    let key = {
        let db = &s.db;
        if db.is_initialized().await? {
            let (salt, token) = db
                .get_meta()
                .await?
                .ok_or_else(|| "vault_meta row missing".to_string())?;
            crypto::unlock_vault_crypto(password_bytes, &salt, &token)?
        } else {
            let (salt, token, k) = crypto::init_vault_crypto(password_bytes)?;
            db.init_vault(&salt, &token).await?;
            k
        }
    };

    s.key = Some(Zeroizing::new(key));
    s.touch();

    let raw = s.db.list_items().await?;
    let items: Vec<VaultItem> = raw
        .into_iter()
        .filter_map(|(id, _, data, _)| decrypt_item(&key, id, &data).ok())
        .collect();

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
pub async fn vault_unlock(
    password: String,
    state: State<'_, SharedState>,
) -> Result<UnlockPayload, String> {
    let mut s = state.lock().await;
    do_unlock(password.as_bytes(), &mut s).await
}

/// Internal lock used by both the Tauri command and the background auto-lock task.
pub async fn lock_vault(shared: &SharedState) {
    let mut s = shared.lock().await;
    s.key = None;
    s.last_activity = None;
}

#[tauri::command]
pub async fn vault_lock(state: State<'_, SharedState>) -> Result<(), String> {
    lock_vault(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn vault_get_items(state: State<'_, SharedState>) -> Result<Vec<VaultItem>, String> {
    let mut s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?.clone();
    s.touch();
    let raw = s.db.list_items().await?;
    Ok(raw
        .into_iter()
        .filter_map(|(id, _, data, _)| decrypt_item(&key, id, &data).ok())
        .collect())
}

#[tauri::command]
pub async fn vault_save_item(
    item: VaultItem,
    state: State<'_, SharedState>,
) -> Result<VaultItem, String> {
    let mut s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?.clone();
    s.touch();
    let encrypted = encrypt_item(&key, &item)?;
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
    let mut s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.touch();
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
    s.touch();

    Ok(())
}

#[tauri::command]
pub async fn vault_wipe(state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().await;
    s.key = None;
    s.last_activity = None;
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
    let mut s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.touch();

    // Generar token de 32 bytes hex
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();

    // Guardar en DB
    s.db.set_setting("mcp_token", &token).await?;

    // Write token file for the MCP binary.
    // On Windows, %APPDATA% inherits user-only NTFS ACLs; no further ACL work needed.
    // On Unix, restrict to 0o600 (owner read/write only) after the write.
    let app_dir = app.path().app_data_dir()
        .map_err(|e| format!("path error: {e}"))?;
    let token_path = app_dir.join("mcp_token");
    std::fs::write(&token_path, &token)
        .map_err(|e| format!("write mcp token file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set mcp token file permissions: {e}"))?;
    }

    Ok(token)
}

#[tauri::command]
pub async fn vault_get_mcp_token(
    state: State<'_, SharedState>,
) -> Result<Option<String>, String> {
    let mut s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.touch();
    s.db.get_setting("mcp_token").await
}

// ─── Backup types ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct BackupItem {
    id: i64,
    item_type: String,
    /// Raw AES-GCM encrypted blob from the DB (hex-encoded nonce || ciphertext).
    data: String,
    created: String,
}

#[derive(Serialize, Deserialize)]
struct BackupFile {
    version: u32,
    created_at: u64,
    /// Hex-encoded Argon2id salt used to derive the vault key.
    salt: String,
    /// AES-GCM verify token that proves the master password is correct.
    token: String,
    items: Vec<BackupItem>,
    categories: Vec<serde_json::Value>,
}

// ─── Backup commands ──────────────────────────────────────────────────────────

/// Export all vault items and categories to a `.cenvbak` file.
/// The backup preserves the vault's existing salt and verify token so that
/// restoring only requires the original master password — no separate backup password.
/// Returns the number of items written.
#[tauri::command]
pub async fn vault_export_backup(
    path: String,
    state: State<'_, SharedState>,
) -> Result<usize, String> {
    let mut s = state.lock().await;
    s.key.as_ref().ok_or("vault is locked")?;
    s.touch();

    // Read vault crypto material
    let (salt, token) = s
        .db
        .get_meta()
        .await?
        .ok_or("vault_meta missing")?;

    // Read all raw (already-encrypted) item rows
    let raw_items = s.db.list_items().await?;
    let items: Vec<BackupItem> = raw_items
        .iter()
        .map(|(id, item_type, data, created)| BackupItem {
            id: *id,
            item_type: item_type.clone(),
            data: data.clone(),
            created: created.clone(),
        })
        .collect();
    let item_count = items.len();

    // Read categories as generic JSON values
    let db_cats = s.db.list_categories().await?;
    let categories: Vec<serde_json::Value> = db_cats
        .iter()
        .map(|c| serde_json::json!({ "id": c.cid, "name": c.name, "color": c.color }))
        .collect();

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let backup = BackupFile {
        version: 1,
        created_at,
        salt,
        token,
        items,
        categories,
    };

    let json = serde_json::to_string_pretty(&backup)
        .map_err(|e| format!("serialize backup: {e}"))?;

    std::fs::write(&path, json).map_err(|e| format!("write backup: {e}"))?;

    Ok(item_count)
}

/// Import a `.cenvbak` file by filesystem path.
///
/// - `merge = false`: wipes the current vault and restores the backup exactly,
///   keeping the original crypto material so the same master password applies.
/// - `merge = true`: re-encrypts each imported item with the *current* vault key
///   and upserts into the live vault. Categories are merged (no duplicates by id).
///
/// Returns the number of items restored.
#[tauri::command]
pub async fn vault_import_backup(
    path: String,
    master_password: String,
    merge: bool,
    state: State<'_, SharedState>,
) -> Result<usize, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read backup: {e}"))?;
    do_restore_backup(&json, &master_password, merge, &state).await
}

/// Import a `.cenvbak` by passing its JSON content directly.
/// Used by the frontend file picker where only file content (not the path) is accessible.
#[tauri::command]
pub async fn vault_import_backup_data(
    data: String,
    master_password: String,
    merge: bool,
    state: State<'_, SharedState>,
) -> Result<usize, String> {
    do_restore_backup(&data, &master_password, merge, &state).await
}

async fn do_restore_backup(
    json: &str,
    master_password: &str,
    merge: bool,
    state: &State<'_, SharedState>,
) -> Result<usize, String> {
    let backup: BackupFile = serde_json::from_str(json)
        .map_err(|e| format!("parse backup: {e}"))?;

    if backup.version != 1 {
        return Err(format!("unsupported backup version: {}", backup.version));
    }

    // Verify the master password against the backup's crypto material.
    let backup_key = crypto::unlock_vault_crypto(
        master_password.as_bytes(),
        &backup.salt,
        &backup.token,
    )
    .map_err(|_| "incorrect master password for this backup".to_string())?;

    let mut s = state.lock().await;

    if !merge {
        // Wipe the current vault and restore the backup's crypto material verbatim.
        // After init_vault the in-memory key equals the backup key.
        s.key = None;
        s.db.wipe_and_reset().await?;
        s.db.init_vault(&backup.salt, &backup.token).await?;
        s.key = Some(Zeroizing::new(backup_key));
        s.touch();

        // Insert items using their original encrypted blobs (already keyed with backup_key).
        for item in &backup.items {
            s.db
                .upsert_item(0, &item.item_type, &item.data, &item.created)
                .await?;
        }

        // Restore categories wholesale.
        let db_cats: Vec<DbCategory> = backup
            .categories
            .iter()
            .filter_map(|v| {
                Some(DbCategory {
                    cid: v.get("id")?.as_str()?.to_string(),
                    name: v.get("name")?.as_str()?.to_string(),
                    color: v.get("color")?.as_str()?.to_string(),
                })
            })
            .collect();
        s.db.save_categories(&db_cats).await?;
    } else {
        // Merge: re-encrypt each item from backup_key → current vault key.
        let current_key = s.key.as_ref().ok_or("vault is locked")?.clone();
        s.touch();

        for item in &backup.items {
            let plaintext = crypto::decrypt(&backup_key, &item.data)
                .map_err(|e| format!("decrypt backup item {}: {e}", item.id))?;
            let new_data = crypto::encrypt(&current_key, &plaintext)
                .map_err(|e| format!("re-encrypt item {}: {e}", item.id))?;
            s.db
                .upsert_item(0, &item.item_type, &new_data, &item.created)
                .await?;
        }

        // Merge categories: keep existing, append any new ids from the backup.
        let existing_cats = s.db.list_categories().await?;
        let existing_ids: std::collections::HashSet<String> =
            existing_cats.iter().map(|c| c.cid.clone()).collect();

        let mut merged = existing_cats;
        for v in &backup.categories {
            let cid = match v.get("id").and_then(|x| x.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            if !existing_ids.contains(&cid) {
                merged.push(DbCategory {
                    cid,
                    name: v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    color: v.get("color").and_then(|x| x.as_str()).unwrap_or("#888").to_string(),
                });
            }
        }
        s.db.save_categories(&merged).await?;
    }

    Ok(backup.items.len())
}

// ─── Import from password managers ───────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct ImportItem {
    pub name: String,
    pub value: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub item_type: String,
}

#[derive(Deserialize)]
pub struct ParseImportArgs {
    pub content: String,
    pub format: String,
}

/// Parse file content into a preview list of ImportItems without touching vault state.
#[tauri::command]
pub async fn vault_parse_import(args: ParseImportArgs) -> Result<Vec<ImportItem>, String> {
    let items = match args.format.as_str() {
        "env"       => import::parse_env_file(&args.content),
        "bitwarden" => import::parse_bitwarden_csv(&args.content),
        "1password" => import::parse_1password_csv(&args.content),
        "csv"       => import::parse_csv_generic(&args.content),
        other       => return Err(format!("unknown import format: {other}")),
    };
    Ok(items)
}

/// Import a list of items into the vault, skipping any whose name already exists.
/// Returns the count of items actually inserted.
#[tauri::command]
pub async fn vault_import_items(
    items: Vec<ImportItem>,
    state: State<'_, SharedState>,
) -> Result<usize, String> {
    let mut s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?.clone();
    s.touch();
    let key = &key;

    // Build the set of existing item names to detect duplicates.
    let raw = s.db.list_items().await?;
    let existing_names: std::collections::HashSet<String> = raw
        .iter()
        .filter_map(|(id, _, data, _)| {
            decrypt_item(key, *id, data).ok().and_then(|v| v.name)
        })
        .collect();

    // Epoch-seconds timestamp shared by all items in this import batch.
    let epoch_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let now = epoch_to_iso8601(epoch_secs);

    let mut inserted = 0usize;

    for imp in items {
        if existing_names.contains(&imp.name) {
            continue;
        }

        let vault_item = VaultItem {
            id: 0,
            item_type: imp.item_type.clone(),
            name: Some(imp.name),
            value: imp.value,
            username: imp.username,
            password: imp.password,
            url: imp.url,
            notes: imp.notes,
            title: None,
            description: None,
            command: None,
            shell: None,
            content: None,
            categories: Vec::new(),
            created: now.clone(),
        };

        let encrypted = encrypt_item(key, &vault_item)?;
        s.db
            .upsert_item(0, &vault_item.item_type, &encrypted, &vault_item.created)
            .await?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Format Unix epoch seconds as an ISO-8601 UTC string: "2006-01-02T15:04:05Z".
fn epoch_to_iso8601(secs: u64) -> String {
    let sec  = secs % 60;
    let min  = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let days = secs / 86400;
    let (year, month, day) = days_since_epoch_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days-since-Unix-epoch (1970-01-01) to (year, month, day).
/// Uses the algorithm from http://howardhinnant.github.io/date_algorithms.html.
fn days_since_epoch_to_ymd(mut days: u64) -> (u64, u64, u64) {
    days += 719468;
    let era = days / 146097;
    let doe = days % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y   = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp  = (5 * doy + 2) / 153;
    let d   = doy - (153 * mp + 2) / 5 + 1;
    let m   = if mp < 10 { mp + 3 } else { mp - 9 };
    let y   = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ─── Biometric commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn biometric_check(state: State<'_, SharedState>) -> Result<String, String> {
    let _ = state; // state not needed; availability is system-wide
    let status = biometric::check_availability().await;
    Ok(status.as_str().to_string())
}

#[tauri::command]
pub async fn biometric_is_enrolled(state: State<'_, SharedState>) -> Result<bool, String> {
    let s = state.lock().await;
    let blob = s.db.get_setting("biometric_blob").await?;
    Ok(blob.map(|v| !v.is_empty()).unwrap_or(false))
}

#[tauri::command]
pub async fn biometric_enroll(
    password: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    // 1. Verify the supplied password against the stored vault meta.
    {
        let s = state.lock().await;
        let (salt, token) = s
            .db
            .get_meta()
            .await?
            .ok_or_else(|| "vault not initialized".to_string())?;
        crypto::unlock_vault_crypto(password.as_bytes(), &salt, &token)?;
    }

    // 2. Request Windows Hello consent before storing anything.
    let verified = biometric::request_verification("Enroll CryptEnv biometric unlock").await?;
    if !verified {
        return Err("Windows Hello verification was not completed".to_string());
    }

    // 3. DPAPI-protect the password bytes, then hex-encode for DB storage.
    #[cfg(target_os = "windows")]
    let hex_blob = {
        use zeroize::Zeroizing;
        let pw_bytes = Zeroizing::new(password.into_bytes());
        let blob = biometric::dpapi_protect(&pw_bytes)?;
        crypto::hex_encode(&blob)
    };

    #[cfg(not(target_os = "windows"))]
    let hex_blob: String = {
        return Err("biometric unlock is not available on this platform".to_string());
        #[allow(unreachable_code)]
        String::new()
    };

    let s = state.lock().await;
    s.db.set_setting("biometric_blob", &hex_blob).await?;
    Ok(())
}

#[tauri::command]
pub async fn biometric_unlock(state: State<'_, SharedState>) -> Result<UnlockPayload, String> {
    // 1. Retrieve the stored DPAPI blob.
    let hex_blob = {
        let s = state.lock().await;
        s.db
            .get_setting("biometric_blob")
            .await?
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "biometric unlock is not enrolled".to_string())?
    };

    // 2. Request Windows Hello consent.
    let verified = biometric::request_verification("Unlock CryptEnv vault").await?;
    if !verified {
        return Err("Windows Hello verification was not completed".to_string());
    }

    // 3. Decode + DPAPI-unprotect to recover the master password bytes.
    #[cfg(target_os = "windows")]
    let password_bytes = {
        let raw = crypto::hex_decode(&hex_blob)
            .map_err(|_| "biometric blob is corrupt".to_string())?;
        biometric::dpapi_unprotect(&raw)?
    };

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hex_blob;
        return Err("biometric unlock is not available on this platform".to_string());
    }

    // 4. Unlock using the recovered password, same path as vault_unlock.
    #[cfg(target_os = "windows")]
    {
        let mut s = state.lock().await;
        do_unlock(&password_bytes, &mut s).await
    }
}

#[tauri::command]
pub async fn biometric_disable(state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().await;
    s.db.set_setting("biometric_blob", "").await?;
    Ok(())
}
