use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub mod api;
pub mod biometric;
pub mod cli;
pub mod crypto;
pub mod db;
pub mod mcp;
pub mod share;
pub mod tls;
pub mod vault;

use vault::{
    biometric_check, biometric_disable, biometric_enroll, biometric_is_enrolled, biometric_unlock,
    lock_vault, vault_change_password, vault_delete_item, vault_export_backup,
    vault_generate_mcp_token, vault_get_categories, vault_get_items, vault_get_mcp_token,
    vault_get_settings, vault_import_backup, vault_import_backup_data, vault_import_items,
    vault_is_setup, vault_lock, vault_parse_import, vault_save_categories, vault_save_item,
    vault_save_settings, vault_unlock, vault_wipe, SharedState, VaultState,
};
use vault::share_commands::{
    share_cancel, share_confirm_fingerprint, share_export_file, share_import_file,
    share_poll_status, share_start_receive, share_start_send, SharedShareState,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Multiple crates in the dependency tree enable different rustls crypto
    // providers (ring + aws-lc-rs), so rustls cannot auto-select one.
    // Install ring explicitly before any TLS code runs.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");

            let db_path = app_dir.join("vault.db");
            let db_path_str = db_path.to_str().expect("invalid db path");

            let db = tauri::async_runtime::block_on(db::VaultDb::open(db_path_str))
                .expect("failed to open vault database");

            let state: SharedState =
                std::sync::Arc::new(tokio::sync::Mutex::new(VaultState::new(db)));
            app.manage(state.clone());

            let share_state: SharedShareState =
                std::sync::Arc::new(share::ShareState::new());
            app.manage(share_state);

            let api_state = state.clone();
            let api_app_dir = app_dir.clone();
            tauri::async_runtime::spawn(api::start_server(api_state, api_app_dir));

            // Background auto-lock: every 30 s, check idle time against the
            // configured timeout. Timeout = 0 means "never lock".
            let auto_lock_state = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(30));
                // The first tick fires immediately; skip it so we don't lock
                // right at startup before the user has a chance to unlock.
                interval.tick().await;

                loop {
                    interval.tick().await;

                    // Acquire lock, compute whether we should auto-lock, then
                    // release before calling lock_vault (which re-acquires).
                    let should_lock = {
                        let s = auto_lock_state.lock().await;

                        // Only relevant while the vault is unlocked.
                        let last = match s.last_activity {
                            Some(t) => t,
                            None => continue,
                        };

                        let timeout_mins: u64 = s
                            .db
                            .get_setting("auto_lock_timeout")
                            .await
                            .ok()
                            .flatten()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(5);

                        // 0 means "never auto-lock".
                        if timeout_mins == 0 {
                            continue;
                        }

                        last.elapsed() >= std::time::Duration::from_secs(timeout_mins * 60)
                    };

                    if should_lock {
                        lock_vault(&auto_lock_state).await;
                    }
                }
            });

            let handle = app.handle().clone();
            app.handle()
                .global_shortcut()
                .on_shortcut("Ctrl+Alt+Z", move |_app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(window) = handle.get_webview_window("main") {
                            let visible = window.is_visible().unwrap_or(false);
                            if visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_is_setup,
            vault_unlock,
            vault_lock,
            vault_get_items,
            vault_save_item,
            vault_delete_item,
            vault_get_categories,
            vault_save_categories,
            vault_get_settings,
            vault_save_settings,
            vault_change_password,
            vault_wipe,
            vault_generate_mcp_token,
            vault_get_mcp_token,
            vault_export_backup,
            vault_import_backup,
            vault_import_backup_data,
            vault_parse_import,
            vault_import_items,
            biometric_check,
            biometric_is_enrolled,
            biometric_enroll,
            biometric_unlock,
            biometric_disable,
            share_start_send,
            share_start_receive,
            share_poll_status,
            share_confirm_fingerprint,
            share_cancel,
            share_export_file,
            share_import_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
