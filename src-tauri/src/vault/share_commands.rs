use std::sync::Arc;
use tauri::State;
use serde::Serialize;

use crate::share::{self, ShareState, ShareSessionState};
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
