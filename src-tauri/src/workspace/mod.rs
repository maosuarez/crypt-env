use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

use crate::db::DbWorkspaceVar;
use crate::vault::SharedState;

// ─── Frontend-facing types ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkspaceVar {
    #[serde(default)]
    pub id: i64,
    pub key: String,
    #[serde(rename = "itemId", skip_serializing_if = "Option::is_none")]
    pub item_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Workspace {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub paths: Vec<String>,
    pub template: String,
    pub created: String,
    pub updated: String,
    pub vars: Vec<WorkspaceVar>,
}

#[derive(Deserialize)]
pub struct WorkspaceInput {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub template: String,
    pub vars: Vec<WorkspaceVar>,
}

#[derive(Serialize)]
pub struct InjectResult {
    pub paths: Vec<String>,
    pub written: Vec<String>,
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn workspace_list(state: State<'_, SharedState>) -> Result<Vec<Workspace>, String> {
    let s = state.lock().await;
    let workspaces = s.db.list_workspaces().await?;
    let mut result = Vec::with_capacity(workspaces.len());
    for ws in workspaces {
        let db_vars = s.db.get_workspace_vars(ws.id).await?;
        let vars = db_vars
            .into_iter()
            .map(|v| WorkspaceVar {
                id: v.id,
                key: v.key,
                item_id: v.item_id,
                literal: v.literal,
            })
            .collect();
        result.push(Workspace {
            id: ws.id,
            name: ws.name,
            description: ws.description,
            paths: ws.paths,
            template: ws.template,
            created: ws.created,
            updated: ws.updated,
            vars,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn workspace_save(
    state: State<'_, SharedState>,
    workspace: WorkspaceInput,
) -> Result<i64, String> {
    let s = state.lock().await;
    let ws_id = s
        .db
        .upsert_workspace(
            workspace.id,
            &workspace.name,
            workspace.description.as_deref(),
            &workspace.template,
        )
        .await?;

    s.db.set_workspace_paths(ws_id, &workspace.paths).await?;

    let db_vars: Vec<DbWorkspaceVar> = workspace
        .vars
        .into_iter()
        .map(|v| DbWorkspaceVar {
            id: 0,
            workspace_id: ws_id,
            key: v.key,
            item_id: v.item_id,
            literal: v.literal,
        })
        .collect();

    s.db.set_workspace_vars(ws_id, &db_vars).await?;
    Ok(ws_id)
}

#[tauri::command]
pub async fn workspace_delete(
    state: State<'_, SharedState>,
    id: i64,
) -> Result<(), String> {
    let s = state.lock().await;
    s.db.delete_workspace(id).await
}

/// Decrypt referenced vault items and write KEY=VALUE pairs into each .env at workspace.paths.
/// Merges with existing file content (existing keys not in the workspace are preserved).
#[tauri::command]
pub async fn workspace_inject(
    state: State<'_, SharedState>,
    id: i64,
) -> Result<InjectResult, String> {
    let s = state.lock().await;

    let key = s.key.as_ref().ok_or("vault is locked")?;
    let vault_key: [u8; 32] = **key;

    let workspaces = s.db.list_workspaces().await?;
    let ws = workspaces
        .into_iter()
        .find(|w| w.id == id)
        .ok_or("workspace not found")?;

    if ws.paths.is_empty() {
        return Err("workspace has no paths configured".into());
    }

    let vars = s.db.get_workspace_vars(id).await?;

    if vars.is_empty() {
        return Ok(InjectResult { paths: ws.paths, written: vec![] });
    }

    // Decrypt vault items needed
    let raw_items = s.db.list_items().await?;
    let mut item_values: HashMap<i64, String> = HashMap::new();
    for (item_id, _, data, _) in &raw_items {
        let json = crate::crypto::decrypt(&vault_key, data)?;
        let item: crate::vault::VaultItem =
            serde_json::from_slice(&json).map_err(|e| format!("parse item: {e}"))?;
        let value = item.value
            .or(item.password)
            .or(item.content)
            .unwrap_or_default();
        item_values.insert(*item_id, value);
    }

    // Build key → value map for this workspace
    let mut inject_map: HashMap<String, String> = HashMap::new();
    for v in &vars {
        let value = if let Some(iid) = v.item_id {
            item_values.get(&iid).cloned().unwrap_or_default()
        } else {
            v.literal.clone().unwrap_or_default()
        };
        inject_map.insert(v.key.clone(), value);
    }

    // Write inject_map into each configured .env path
    let mut written_keys: Vec<String> = Vec::new();
    let mut written_once = false;

    for path in &ws.paths {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let mut lines: Vec<String> = existing.lines().map(String::from).collect();
        let mut updated_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut written: Vec<String> = Vec::new();

        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.contains('=') {
                continue;
            }
            let eq_pos = trimmed.find('=').unwrap();
            let existing_key = trimmed[..eq_pos].trim().to_string();
            if let Some(new_val) = inject_map.get(&existing_key) {
                *line = format!("{}={}", existing_key, new_val);
                updated_keys.insert(existing_key.clone());
                written.push(existing_key);
            }
        }

        for (k, v) in &inject_map {
            if !updated_keys.contains(k) {
                lines.push(format!("{}={}", k, v));
                written.push(k.clone());
            }
        }

        let content = lines.join("\n") + "\n";
        std::fs::write(path, content).map_err(|e| format!("write .env ({}): {e}", path))?;

        if !written_once {
            written.sort();
            written_keys = written;
            written_once = true;
        }
    }

    Ok(InjectResult { paths: ws.paths, written: written_keys })
}

#[tauri::command]
pub async fn workspace_pick_env_path() -> Result<Option<String>, String> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Select .env file location")
            .add_filter("Env files", &["env"])
            .add_filter("All files", &["*"])
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(result.map(|p| p.to_string_lossy().into_owned()))
}

// ─── Export / Import ──────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct ExportedWorkspaceVar {
    pub key: String,
    pub literal: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportedWorkspace {
    pub version: u8,
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub vars: Vec<ExportedWorkspaceVar>,
}

#[tauri::command]
pub async fn workspace_export(
    workspace_id: i64,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let s = state.lock().await;
    let workspaces = s.db.list_workspaces().await?;
    let ws = workspaces
        .into_iter()
        .find(|w| w.id == workspace_id)
        .ok_or("workspace not found")?;
    let db_vars = s.db.get_workspace_vars(workspace_id).await?;

    let exported = ExportedWorkspace {
        version: 1,
        name: ws.name.clone(),
        description: ws.description.clone(),
        template: ws.template.clone(),
        vars: db_vars.into_iter().map(|v| ExportedWorkspaceVar {
            key: v.key,
            literal: if v.item_id.is_some() { None } else { v.literal },
        }).collect(),
    };

    let json = serde_json::to_vec_pretty(&exported).map_err(|e| e.to_string())?;

    let safe_name = ws.name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    let default_name = format!("{}.cryptenv-ws", safe_name);

    let path = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("CryptEnv Workspace", &["cryptenv-ws"])
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "cancelled".to_string())?;

    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn workspace_import() -> Result<ExportedWorkspace, String> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("CryptEnv Workspace", &["cryptenv-ws"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "cancelled".to_string())?;

    let json = std::fs::read(&path).map_err(|e| e.to_string())?;
    let ws: ExportedWorkspace = serde_json::from_slice(&json).map_err(|e| format!("invalid file: {e}"))?;
    Ok(ws)
}
