use clap::{Args, Subcommand};
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use serde::Deserialize;

use crate::client::{authenticated_delete, authenticated_get, authenticated_post, CliError, API_BASE};

// ─── CLI argument structs ─────────────────────────────────────────────────────

#[derive(Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub cmd: ProjectCmd,
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// List all projects with their environments
    List,
    /// Inject an environment's vars into its configured .env path(s)
    Inject {
        /// Environment ID
        #[arg(long, conflicts_with_all = ["project", "environment"])]
        id: Option<i64>,
        /// Project name (case-insensitive, used together with --environment)
        #[arg(long, requires = "environment")]
        project: Option<String>,
        /// Environment name within the project (case-insensitive)
        #[arg(long, requires = "project")]
        environment: Option<String>,
    },
    /// Delete a project (and all its environments) by ID
    Delete {
        /// Project ID to delete
        #[arg(long)]
        id: i64,
    },
    /// Delete a single environment by ID
    DeleteEnv {
        /// Environment ID to delete
        #[arg(long)]
        id: i64,
    },
}

// ─── Response types ───────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct EnvironmentVar {
    key: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct EnvironmentSummary {
    id: i64,
    name: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    vars: Vec<EnvironmentVar>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct ProjectSummary {
    id: i64,
    name: String,
    template: String,
    #[serde(default)]
    environments: Vec<EnvironmentSummary>,
}

#[derive(Deserialize, Debug)]
struct InjectResult {
    paths: Vec<String>,
    written: Vec<String>,
}

// ─── Entry point ──────────────────────────────────────────────────────────────

pub fn run(args: ProjectArgs) -> Result<(), CliError> {
    match args.cmd {
        ProjectCmd::List => run_list(),
        ProjectCmd::Inject { id, project, environment } => run_inject(id, project, environment),
        ProjectCmd::Delete { id } => run_delete(id),
        ProjectCmd::DeleteEnv { id } => run_delete_env(id),
    }
}

// ─── List ─────────────────────────────────────────────────────────────────────

fn fetch_projects() -> Result<Vec<ProjectSummary>, CliError> {
    let resp = authenticated_get(&format!("{API_BASE}/projects"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("list projects failed: {text}")));
    }

    resp.json().map_err(|e| CliError::Api(e.to_string()))
}

fn run_list() -> Result<(), CliError> {
    let projects = fetch_projects()?;

    if projects.is_empty() {
        println!("No projects found.");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Project", "Template", "Environment", "Paths", "Vars"]);

    for p in &projects {
        if p.environments.is_empty() {
            table.add_row(vec![p.name.clone(), p.template.clone(), "(none)".into(), "(none)".into(), "0".into()]);
            continue;
        }
        for e in &p.environments {
            table.add_row(vec![
                p.name.clone(),
                p.template.clone(),
                e.name.clone(),
                if e.paths.is_empty() { "(none)".into() } else { e.paths.join(", ") },
                e.vars.len().to_string(),
            ]);
        }
    }

    println!("{table}");
    Ok(())
}

// ─── Inject ───────────────────────────────────────────────────────────────────

fn run_inject(id: Option<i64>, project: Option<String>, environment: Option<String>) -> Result<(), CliError> {
    // Resolve environment ID: use --id directly, or look up by --project/--environment
    let environment_id = match (id, project, environment) {
        (Some(i), _, _) => i,
        (None, Some(p), Some(e)) => resolve_environment_id(&p, &e)?,
        _ => {
            return Err(CliError::Api(
                "provide --id, or both --project and --environment to identify the environment".into(),
            ));
        }
    };

    let resp = authenticated_post(
        &format!("{API_BASE}/environments/{environment_id}/inject"),
        &serde_json::json!({}),
    )?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(format!("environment {environment_id}")));
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

/// Find the environment ID matching a project name + environment name (both case-insensitive).
fn resolve_environment_id(project: &str, environment: &str) -> Result<i64, CliError> {
    let projects = fetch_projects()?;
    let project_lower = project.to_lowercase();
    let env_lower = environment.to_lowercase();

    projects
        .into_iter()
        .find(|p| p.name.to_lowercase() == project_lower)
        .and_then(|p| p.environments.into_iter().find(|e| e.name.to_lowercase() == env_lower))
        .map(|e| e.id)
        .ok_or_else(|| CliError::NotFound(format!("{project} / {environment}")))
}

// ─── Delete ───────────────────────────────────────────────────────────────────

fn run_delete(id: i64) -> Result<(), CliError> {
    let resp = authenticated_delete(&format!("{API_BASE}/projects/{id}"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(format!("project {id}")));
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("delete project failed: {text}")));
    }

    println!("Project {id} deleted.");
    Ok(())
}

fn run_delete_env(id: i64) -> Result<(), CliError> {
    let resp = authenticated_delete(&format!("{API_BASE}/environments/{id}"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(format!("environment {id}")));
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("delete environment failed: {text}")));
    }

    println!("Environment {id} deleted.");
    Ok(())
}
