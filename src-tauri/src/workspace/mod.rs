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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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
    pub path: Option<String>,
    pub template: String,
    pub vars: Vec<WorkspaceVar>,
}

#[derive(Serialize)]
pub struct InjectResult {
    pub path: String,
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
            path: ws.path,
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
            workspace.path.as_deref(),
            &workspace.template,
        )
        .await?;

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

/// Decrypt referenced vault items and write KEY=VALUE pairs into the .env at workspace.path.
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

    let path = ws.path.ok_or("workspace has no path configured")?;
    let vars = s.db.get_workspace_vars(id).await?;

    if vars.is_empty() {
        return Ok(InjectResult { path, written: vec![] });
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

    // Read existing .env, merge, write back
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(String::from).collect();
    let mut written: Vec<String> = Vec::new();
    let mut updated_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Update existing lines
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

    // Append new keys not already in the file
    for (k, v) in &inject_map {
        if !updated_keys.contains(k) {
            lines.push(format!("{}={}", k, v));
            written.push(k.clone());
        }
    }

    let content = lines.join("\n") + "\n";
    std::fs::write(&path, content).map_err(|e| format!("write .env: {e}"))?;

    written.sort();
    Ok(InjectResult { path, written })
}
