use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub mod api;
pub mod cli;
pub mod crypto;
pub mod db;
pub mod mcp;
pub mod vault;

use vault::{
    vault_change_password, vault_delete_item, vault_export_backup, vault_generate_mcp_token,
    vault_get_categories, vault_get_items, vault_get_mcp_token, vault_get_settings,
    vault_import_backup, vault_import_backup_data, vault_import_items, vault_is_setup, vault_lock,
    vault_parse_import, vault_save_categories, vault_save_item, vault_save_settings, vault_unlock,
    vault_wipe, SharedState, VaultState,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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

            let api_state = state.clone();
            tauri::async_runtime::spawn(api::start_server(api_state));

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
