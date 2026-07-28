//! Project/environment scope resolution shared by every CLI command that
//! reads or writes vault items linked to a specific project + environment
//! (the API now requires this scope on every such endpoint).
//!
//! Resolution order, per field (`project`, `environment`):
//!   1. The command's own `--project` / `--env` flag, if given.
//!   2. `crypt-env.json`, found by searching from the current directory
//!      upward (same convention as `.git`/`package.json` discovery) — its
//!      `project` and optional `environment` fields.
//!   3. Fallback: project name = current working directory's folder name;
//!      environment = that project's default environment (`isDefault`),
//!      looked up via `GET /projects`.
//!
//!      If the project doesn't exist on the server yet, only callers that
//!      pass `allow_create = true` (currently: `add`, including its
//!      `--file` bulk-import mode) may create it via `POST /projects` —
//!      every other (read-only or non-`add`) command gets a clear error
//!      instead, since silently mutating server state on e.g. `list` or
//!      `search` is surprising and, without care, races into duplicate
//!      projects. The server enforces a case-insensitive unique constraint
//!      on project names, so a lost race on creation is recovered by
//!      re-fetching and reusing whatever project actually ended up with
//!      that name — see [`default_environment_for`].
//!
//! Every scoped command calls [`resolve`] once and appends the result to
//! its request URLs via [`ResolvedScope::append_query`] — no command
//! duplicates the crypt-env.json search or the cwd-derivation fallback.

use serde::Deserialize;
use std::path::Path;

use crate::client::{self, CliError, API_BASE};

pub const CONFIG_FILE_NAME: &str = "crypt-env.json";

/// Schema for `crypt-env.json`. `project` is required; `environment` is an
/// optional default so a repo can pin its environment without every command
/// needing `--env`.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct ProjectConfig {
    pub project: String,
    #[serde(default)]
    pub environment: Option<String>,
}

pub struct ResolvedScope {
    pub project: String,
    pub environment: String,
}

impl ResolvedScope {
    /// Appends `project`/`environment` query params onto `url`, respecting
    /// whether it already has a `?`.
    pub fn append_query(&self, url: &str) -> String {
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{}", self.to_query_string())
    }

    /// `project=<enc>&environment=<enc>`, without a leading `?`/`&` — for
    /// callers (like `client.rs`) that build the query string themselves.
    pub fn to_query_string(&self) -> String {
        format!(
            "project={}&environment={}",
            client::urlencod(&self.project),
            client::urlencod(&self.environment)
        )
    }
}

/// Searches for `crypt-env.json` starting at `start` and walking up to the
/// filesystem root, returning the parsed config from the first file found.
///
/// A file that exists but is malformed is a hard error (not silently
/// skipped) — `doctor` reuses this same parse path to validate the config.
pub fn find_project_config(start: &Path) -> Result<Option<ProjectConfig>, CliError> {
    let mut dir = Some(start.to_path_buf());
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Ok(Some(parse_project_config(&candidate)?));
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    Ok(None)
}

/// Parses and validates a `crypt-env.json` file at `path`. Shared by
/// [`find_project_config`] and `doctor`'s validation pass.
pub fn parse_project_config(path: &Path) -> Result<ProjectConfig, CliError> {
    let content = std::fs::read_to_string(path)?;
    let config: ProjectConfig = serde_json::from_str(&content).map_err(|e| {
        CliError::Api(format!("{}: invalid JSON — {}", path.display(), e))
    })?;
    if config.project.trim().is_empty() {
        return Err(CliError::Api(format!(
            "{}: 'project' field is required and must not be empty",
            path.display()
        )));
    }
    Ok(config)
}

/// Minimal subset of `GET /projects` needed to find a project by name and
/// read its default environment name.
#[derive(Deserialize)]
struct ProjectLookup {
    name: String,
    environments: Vec<EnvLookup>,
}

#[derive(Deserialize)]
struct EnvLookup {
    name: String,
    #[serde(rename = "isDefault")]
    is_default: bool,
}

fn fetch_projects() -> Result<Vec<ProjectLookup>, CliError> {
    let resp = client::authenticated_get(&format!("{API_BASE}/projects"))?;
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        return Err(CliError::Api(format!("list projects failed: HTTP {}", resp.status())));
    }
    resp.json().map_err(|e| CliError::Api(e.to_string()))
}

/// Picks the environment name to use for a resolved project: its
/// `isDefault` environment, or the first one if somehow none is marked
/// default. Never assumes a literal name like `"default"` — always reads
/// back whatever the server actually has.
fn default_env_name(p: &ProjectLookup) -> Result<String, CliError> {
    if let Some(default_env) = p.environments.iter().find(|e| e.is_default) {
        return Ok(default_env.name.clone());
    }
    if let Some(first) = p.environments.first() {
        return Ok(first.name.clone());
    }
    Err(CliError::Api(format!(
        "project '{}' has no environments configured",
        p.name
    )))
}

/// Returns the default environment name for `project_name`. When the
/// project doesn't exist yet and `allow_create` is true, creates it with
/// minimal defaults; otherwise returns a clear error instead of mutating
/// server state.
fn default_environment_for(project_name: &str, allow_create: bool) -> Result<String, CliError> {
    let projects = fetch_projects()?;
    let name_lower = project_name.to_lowercase();

    if let Some(p) = projects.into_iter().find(|p| p.name.to_lowercase() == name_lower) {
        return default_env_name(&p);
    }

    if !allow_create {
        return Err(CliError::Api(format!(
            "no project found for '{project_name}' — run `crypt-env add` to create one, or pass --project/--env"
        )));
    }

    // Project doesn't exist yet — create it with minimal defaults. The
    // server auto-provisions a default environment for a newly created
    // project (see project::save_project) — its name is read back below
    // rather than assumed.
    let body = serde_json::json!({
        "id": 0,
        "name": project_name,
        "template": "",
        "categories": [],
    });
    let resp = client::authenticated_post(&format!("{API_BASE}/projects"), &body)?;

    // A concurrent invocation may have created the same (case-insensitive)
    // name first — the server enforces a unique constraint and reports
    // that as 409 Conflict rather than a hard failure. Any other non-2xx
    // status is a real failure.
    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CONFLICT {
        return Err(CliError::Api(format!(
            "failed to create project '{project_name}': HTTP {}",
            resp.status()
        )));
    }

    // Whether we created it or lost the race to a concurrent create,
    // re-fetch to read back what actually exists on the server.
    let projects = fetch_projects()?;
    let p = projects
        .into_iter()
        .find(|p| p.name.to_lowercase() == name_lower)
        .ok_or_else(|| {
            CliError::Api(format!("project '{project_name}' was created but could not be found"))
        })?;
    default_env_name(&p)
}

/// Resolves project + environment scope for a scoped command, given its
/// optional `--project`/`--env` flag values. See module docs for the
/// per-field fallback order. `allow_create` gates whether a project missing
/// from the server may be silently created — only `add` (and its `--file`
/// mode) should pass `true`.
pub fn resolve(
    project_flag: Option<&str>,
    env_flag: Option<&str>,
    allow_create: bool,
) -> Result<ResolvedScope, CliError> {
    let cwd = std::env::current_dir()?;

    // Only search the filesystem when a field is actually missing from the
    // flags — one search covers both fields.
    let config = if project_flag.is_none() || env_flag.is_none() {
        find_project_config(&cwd)?
    } else {
        None
    };

    let project = match project_flag {
        Some(p) => p.to_string(),
        None => match &config {
            Some(c) => c.project.clone(),
            None => cwd
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    CliError::Api("cannot determine project name from current directory".into())
                })?,
        },
    };

    let environment = match env_flag {
        Some(e) => e.to_string(),
        None => match config.as_ref().and_then(|c| c.environment.clone()) {
            Some(e) => e,
            None => default_environment_for(&project, allow_create)?,
        },
    };

    Ok(ResolvedScope { project, environment })
}
