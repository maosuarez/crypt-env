use clap::{Args, Subcommand};
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use serde::Deserialize;

use crate::client::{authenticated_delete, authenticated_get, authenticated_post, CliError, API_BASE};

// ─── CLI argument structs ─────────────────────────────────────────────────────

#[derive(Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub cmd: WorkspaceCmd,
}

#[derive(Subcommand)]
pub enum WorkspaceCmd {
    /// List all saved workspaces
    List,
    /// Inject workspace vars into the .env file at the workspace path
    Inject {
        /// Workspace ID
        #[arg(long, conflicts_with = "name")]
        id: Option<i64>,
        /// Workspace name (case-insensitive)
        #[arg(long, conflicts_with = "id")]
        name: Option<String>,
    },
    /// Delete a workspace by ID
    Delete {
        /// Workspace ID to delete
        #[arg(long)]
        id: i64,
    },
}

// ─── Response types ───────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct WorkspaceVar {
    key: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct WorkspaceSummary {
    id: i64,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    template: String,
    #[serde(default)]
    vars: Vec<WorkspaceVar>,
}

#[derive(Deserialize, Debug)]
struct InjectResult {
    paths: Vec<String>,
    written: Vec<String>,
}

// ─── Entry point ──────────────────────────────────────────────────────────────

pub fn run(args: WorkspaceArgs) -> Result<(), CliError> {
    match args.cmd {
        WorkspaceCmd::List => run_list(),
        WorkspaceCmd::Inject { id, name } => run_inject(id, name),
        WorkspaceCmd::Delete { id } => run_delete(id),
    }
}

// ─── List ─────────────────────────────────────────────────────────────────────

fn run_list() -> Result<(), CliError> {
    let resp = authenticated_get(&format!("{API_BASE}/workspaces"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("list workspaces failed: {text}")));
    }

    let workspaces: Vec<WorkspaceSummary> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;

    if workspaces.is_empty() {
        println!("No workspaces found.");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["ID", "Name", "Template", "Paths", "Vars"]);

    for ws in &workspaces {
        table.add_row(vec![
            ws.id.to_string(),
            ws.name.clone(),
            ws.template.clone(),
            if ws.paths.is_empty() { "(none)".into() } else { ws.paths.join(", ") },
            ws.vars.len().to_string(),
        ]);
    }

    println!("{table}");
    Ok(())
}

// ─── Inject ───────────────────────────────────────────────────────────────────

fn run_inject(id: Option<i64>, name: Option<String>) -> Result<(), CliError> {
    // Resolve workspace ID: use --id directly, or look up by --name
    let workspace_id = match (id, name) {
        (Some(i), _) => i,
        (None, Some(n)) => resolve_workspace_id_by_name(&n)?,
        (None, None) => {
            return Err(CliError::Api("provide --id or --name to identify the workspace".into()));
        }
    };

    let resp = authenticated_post(
        &format!("{API_BASE}/workspaces/{workspace_id}/inject"),
        &serde_json::json!({}),
    )?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(format!("workspace {workspace_id}")));
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("inject failed: {text}")));
    }

    let result: InjectResult = resp.json().map_err(|e| CliError::Api(e.to_string()))?;

    for path in &result.paths {
        println!("Injected {} var(s) into: {}", result.written.len(), path);
    }
    for key in &result.written {
        println!("  + {key}");
    }

    Ok(())
}

/// GET /workspaces and find the first workspace matching the given name (case-insensitive).
fn resolve_workspace_id_by_name(name: &str) -> Result<i64, CliError> {
    let resp = authenticated_get(&format!("{API_BASE}/workspaces"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("list workspaces failed: {text}")));
    }

    let workspaces: Vec<WorkspaceSummary> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let name_lower = name.to_lowercase();

    workspaces
        .into_iter()
        .find(|ws| ws.name.to_lowercase() == name_lower)
        .map(|ws| ws.id)
        .ok_or_else(|| CliError::NotFound(name.to_string()))
}

// ─── Delete ───────────────────────────────────────────────────────────────────

fn run_delete(id: i64) -> Result<(), CliError> {
    let resp = authenticated_delete(&format!("{API_BASE}/workspaces/{id}"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(format!("workspace {id}")));
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("delete workspace failed: {text}")));
    }

    println!("Workspace {id} deleted.");
    Ok(())
}
