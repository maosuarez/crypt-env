use std::sync::Arc;
use tauri::State;
use serde::Serialize;

use crate::share::{self, package::PlainItem, relay, ShareState, ShareSessionState};
use crate::vault::SharedState;

pub type SharedShareState = Arc<ShareState>;

#[derive(Serialize)]
pub struct StartSendResponse {
    #[serde(rename = "pairingCode")]
    pub pairing_code: String,
}

#[derive(Serialize)]
pub struct StartReceiveResponse {
    pub fingerprint: String,
}

#[derive(Serialize)]
pub struct PollStatusResponse {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(rename = "receivedNames", skip_serializing_if = "Option::is_none")]
    pub received_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Non-fatal informational note (e.g. Windows Firewall reminder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct ExportFileResponse {
    pub passphrase: String,
}

#[derive(Serialize)]
pub struct ImportFileResponse {
    pub names: Vec<String>,
}

#[tauri::command]
pub async fn share_start_send(
    item_ids: Vec<i64>,
    share_state: State<'_, SharedShareState>,
    vault_state: State<'_, SharedState>,
) -> Result<StartSendResponse, String> {
    let vault_key: [u8; 32] = {
        let guard = vault_state.lock().await;
        let k = guard.key.as_ref().ok_or("vault is locked")?;
        **k
    };

    let pairing_code = share::start_listen_session(
        share_state.inner().clone(),
        item_ids,
        vault_key,
        vault_state.inner().clone(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(StartSendResponse { pairing_code })
}

#[tauri::command]
pub async fn share_start_receive(
    pairing_code: String,
    share_state: State<'_, SharedShareState>,
    vault_state: State<'_, SharedState>,
) -> Result<StartReceiveResponse, String> {
    let vault_key: [u8; 32] = {
        let guard = vault_state.lock().await;
        let k = guard.key.as_ref().ok_or("vault is locked")?;
        **k
    };

    let fingerprint = share::connect_to_peer(
        share_state.inner().clone(),
        pairing_code,
        vault_key,
        vault_state.inner().clone(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(StartReceiveResponse { fingerprint })
}

#[tauri::command]
pub async fn share_poll_status(
    share_state: State<'_, SharedShareState>,
) -> Result<PollStatusResponse, String> {
    let guard = share_state.session.lock().await;
    match guard.as_ref() {
        None => Ok(PollStatusResponse {
            state: "none".to_string(),
            fingerprint: None,
            received_names: None,
            error: None,
            note: None,
        }),
        Some(s) => {
            let error = match &s.state {
                ShareSessionState::Failed(e) => Some(e.clone()),
                _ => None,
            };
            let received_names = match &s.state {
                ShareSessionState::Done if !s.received_names.is_empty() => {
                    Some(s.received_names.clone())
                }
                _ => None,
            };
            Ok(PollStatusResponse {
                state: s.state.as_str().to_string(),
                fingerprint: s.fingerprint.clone(),
                received_names,
                error,
                note: s.note.clone(),
            })
        }
    }
}

#[tauri::command]
pub async fn share_confirm_fingerprint(
    confirmed: bool,
    share_state: State<'_, SharedShareState>,
) -> Result<(), String> {
    share::confirm_fingerprint(share_state.inner(), confirmed)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn share_cancel(
    share_state: State<'_, SharedShareState>,
) -> Result<(), String> {
    // Best-effort cancel — ignore "no active session" errors
    let _ = share::cancel_session(share_state.inner()).await;
    Ok(())
}

#[tauri::command]
pub async fn share_export_file(
    item_ids: Vec<i64>,
    vault_state: State<'_, SharedState>,
) -> Result<ExportFileResponse, String> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_file_name("vault-export.enc")
            .add_filter("Encrypted vault package", &["enc"])
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "file save cancelled".to_string())?;

    let passphrase = share::export_package(&item_ids, &path, vault_state.inner())
        .await
        .map_err(|e| e.to_string())?;

    Ok(ExportFileResponse { passphrase })
}

#[tauri::command]
pub async fn share_import_file(
    passphrase: String,
    vault_state: State<'_, SharedState>,
) -> Result<ImportFileResponse, String> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Encrypted vault package", &["enc"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "file open cancelled".to_string())?;

    let names = share::import_package(&path, &passphrase, vault_state.inner())
        .await
        .map_err(|e| e.to_string())?;

    Ok(ImportFileResponse { names })
}

// ─── Internet relay commands ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct RelayShareResult {
    pub code: String,
    pub passphrase: String,
}

#[tauri::command]
pub async fn share_relay_send(
    item_ids: Vec<i64>,
    vault_state: State<'_, SharedState>,
) -> Result<RelayShareResult, String> {
    let (vault_key, supabase_url, anon_key, plain_items) = {
        let guard = vault_state.lock().await;
        let k = guard.key.as_ref().ok_or("vault is locked")?;
        let vault_key: [u8; 32] = **k;

        let supabase_url = guard
            .db
            .get_setting("relay_supabase_url")
            .await?
            .ok_or("Relay not configured. Set relay_supabase_url in Settings.")?;
        let anon_key = guard
            .db
            .get_setting("relay_supabase_anon_key")
            .await?
            .ok_or("Relay not configured. Set relay_supabase_anon_key in Settings.")?;

        let raw = guard.db.list_items().await?;
        let mut items: Vec<PlainItem> = Vec::new();
        for (id, _, data, _) in &raw {
            if !item_ids.contains(id) {
                continue;
            }
            let json = crate::crypto::decrypt(&vault_key, data)?;
            let item: crate::vault::VaultItem =
                serde_json::from_slice(&json).map_err(|e| format!("parse: {e}"))?;
            items.push(PlainItem {
                item_type: item.item_type,
                name: item.name.unwrap_or_default(),
                value: item.value,
                username: item.username,
                password: item.password,
                url: item.url,
                notes: item.notes,
                category: item.categories.and_then(|v| v.into_iter().next()),
                command: item.command,
            });
        }
        (vault_key, supabase_url, anon_key, items)
    };

    let code = relay::generate_share_code();
    let passphrase = crate::share::crypto::generate_passphrase();

    let relay_key = relay::derive_relay_key(&code, &passphrase).map_err(|e| e.to_string())?;
    let payload = relay::encrypt_items(&plain_items, &relay_key).map_err(|e| e.to_string())?;

    let code_clone = code.clone();
    let url_clone = supabase_url.clone();
    let key_clone = anon_key.clone();
    tokio::task::spawn_blocking(move || {
        relay::relay_upload(&url_clone, &key_clone, &code_clone, &payload)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(RelayShareResult { code, passphrase })
}

#[tauri::command]
pub async fn share_relay_receive(
    code: String,
    passphrase: String,
    vault_state: State<'_, SharedState>,
) -> Result<Vec<String>, String> {
    let (vault_key, supabase_url, anon_key) = {
        let guard = vault_state.lock().await;
        let k = guard.key.as_ref().ok_or("vault is locked")?;
        let vault_key: [u8; 32] = **k;
        let supabase_url = guard
            .db
            .get_setting("relay_supabase_url")
            .await?
            .ok_or("Relay not configured. Set relay_supabase_url in Settings.")?;
        let anon_key = guard
            .db
            .get_setting("relay_supabase_anon_key")
            .await?
            .ok_or("Relay not configured. Set relay_supabase_anon_key in Settings.")?;
        (vault_key, supabase_url, anon_key)
    };

    let relay_key = relay::derive_relay_key(&code, &passphrase).map_err(|e| e.to_string())?;

    let code_clone = code.clone();
    let url_clone = supabase_url.clone();
    let key_clone = anon_key.clone();
    let payload = tokio::task::spawn_blocking(move || {
        relay::relay_download(&url_clone, &key_clone, &code_clone)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let plain_items = relay::decrypt_payload(&payload, &relay_key).map_err(|e| e.to_string())?;

    let url_clone2 = supabase_url.clone();
    let key_clone2 = anon_key.clone();
    let code_clone2 = code.clone();
    let _ = tokio::task::spawn_blocking(move || {
        relay::relay_mark_retrieved(&url_clone2, &key_clone2, &code_clone2)
    })
    .await;

    let guard = vault_state.lock().await;
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let mut names = Vec::new();
    for plain in &plain_items {
        let vault_item = crate::vault::VaultItem {
            id: 0,
            item_type: plain.item_type.clone(),
            name: Some(plain.name.clone()),
            value: plain.value.clone(),
            username: plain.username.clone(),
            password: plain.password.clone(),
            url: plain.url.clone(),
            notes: plain.notes.clone(),
            title: None,
            description: None,
            command: plain.command.clone(),
            shell: None,
            content: None,
            categories: Some(plain.category.iter().cloned().collect()),
            created: now_ts.clone(),
        };
        let json = serde_json::to_vec(&vault_item).map_err(|e| e.to_string())?;
        let encrypted = crate::crypto::encrypt(&vault_key, &json)?;
        guard
            .db
            .upsert_item(0, &vault_item.item_type, &encrypted, &vault_item.created)
            .await?;
        names.push(plain.name.clone());
    }

    Ok(names)
}
