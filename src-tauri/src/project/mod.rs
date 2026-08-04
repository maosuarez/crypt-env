use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::State;

use crate::db::{DbEnvironmentVar, ProjectDeleteImpact, VaultDb};
use crate::envfile;
use crate::vault::SharedState;

// ─── Frontend-facing types ────────────────────────────────────────────────────

/// Every variable is a real vault item now — no more bare literals. `item_id`
/// points into the shared `items` table; ownership (`item_projects`) is
/// granted automatically by `save_environment` below.
#[derive(Serialize, Deserialize, Clone)]
pub struct EnvironmentVar {
    #[serde(default)]
    pub id: i64,
    pub key: String,
    #[serde(rename = "itemId")]
    pub item_id: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Environment {
    #[serde(default)]
    pub id: i64,
    #[serde(rename = "projectId")]
    pub project_id: i64,
    pub name: String,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    pub paths: Vec<String>,
    pub vars: Vec<EnvironmentVar>,
    pub created: String,
    pub updated: String,
}

#[derive(Deserialize)]
pub struct EnvironmentInput {
    #[serde(default)]
    pub id: i64,
    /// `#[serde(default)]` so a missing field produces a friendly 422
    /// (`handle_save_environment` validates `project_id > 0` and that it
    /// references an existing project) instead of a raw JSON-deserialize
    /// rejection.
    #[serde(default, rename = "projectId")]
    pub project_id: i64,
    pub name: String,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub vars: Vec<EnvironmentVar>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Project {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub template: String,
    pub created: String,
    pub updated: String,
    pub environments: Vec<Environment>,
    /// Category NAMES (same convention as `VaultItem.categories`) — reuses
    /// the existing categories table for project tags/language.
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Deserialize)]
pub struct ProjectInput {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub template: String,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Serialize)]
pub struct InjectResult {
    pub paths: Vec<String>,
    pub written: Vec<String>,
    /// Owner-configured paths (`environment.paths[]`) that were `Foreign`
    /// (pre-existing, not carrying the crypt-env marker) at the time of
    /// this inject. Written anyway — configured paths are grandfathered,
    /// never hard-gated (see §4.4 of the issue #8 plan) — but reported so
    /// the caller can see it. A path only ever appears here once: writing
    /// through leaves the marker in place, so the next inject finds it
    /// `Managed` and it drops off this list.
    #[serde(rename = "unmanagedPaths")]
    pub unmanaged_paths: Vec<String>,
    /// `.bak` paths created because a write target was `Foreign` — see
    /// `envfile::commit`. Empty unless a pre-existing unmanaged file was
    /// just overwritten.
    pub backups: Vec<String>,
}

/// Result of `environment_inject_preview` — resolves and inspects paths
/// without decrypting anything or writing a byte. Lets the GUI show a
/// confirm dialog naming the exact files that would be overwritten before
/// the user commits to `overwrite: true`.
#[derive(Serialize)]
pub struct InjectPreview {
    pub paths: Vec<String>,
    pub foreign: Vec<String>,
}

/// Where a write-target path came from, for gating purposes (§4.4 of the
/// issue #8 plan): a path the vault owner saved into `environment.paths[]`
/// through the GUI is pre-consented and only ever reported if unmanaged; a
/// path an API/MCP caller invented on this one request is hard-gated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathOrigin {
    Configured,
    CallerSupplied,
}

// ─── Pure logic (no Tauri/Axum coupling) ──────────────────────────────────────
// Shared by the Tauri commands below and the HTTP handlers in `api::mod`, which
// previously duplicated this logic almost verbatim.

pub async fn list_projects(db: &VaultDb) -> Result<Vec<Project>, String> {
    let db_projects = db.list_projects().await?;
    let mut result = Vec::with_capacity(db_projects.len());
    for p in db_projects {
        let db_envs = db.list_environments(p.id).await?;
        let mut environments = Vec::with_capacity(db_envs.len());
        for e in db_envs {
            let db_vars = db.get_environment_vars(e.id).await?;
            environments.push(Environment {
                id: e.id,
                project_id: e.project_id,
                name: e.name,
                is_default: e.is_default,
                paths: e.paths,
                // Vars still awaiting the post-unlock literal→item migration
                // (item_id not yet set) are skipped rather than exposed broken.
                vars: db_vars
                    .into_iter()
                    .filter_map(|v| v.item_id.map(|item_id| EnvironmentVar { id: v.id, key: v.key, item_id }))
                    .collect(),
                created: e.created,
                updated: e.updated,
            });
        }
        let categories = db.list_project_categories(p.id).await?;
        result.push(Project {
            id: p.id,
            name: p.name,
            description: p.description,
            template: p.template,
            created: p.created,
            updated: p.updated,
            environments,
            categories,
        });
    }
    Ok(result)
}

/// Resolves category NAMES (as sent by the frontend, same convention as
/// `VaultItem.categories`) to their stable ids for storage.
async fn category_names_to_ids(db: &VaultDb, names: &[String]) -> Result<Vec<String>, String> {
    let all = db.list_categories().await?;
    Ok(all
        .into_iter()
        .filter(|c| names.contains(&c.name))
        .map(|c| c.cid)
        .collect())
}

/// Creates (id = 0) or updates (id > 0) a project's metadata. A newly created
/// project always gets one 'default' environment so it's immediately usable.
pub async fn save_project(db: &VaultDb, input: ProjectInput) -> Result<i64, String> {
    let is_new = input.id == 0;
    let project_id = db
        .upsert_project(input.id, &input.name, input.description.as_deref(), &input.template)
        .await?;
    if is_new {
        db.upsert_environment(0, project_id, "default", true).await?;
    }
    let category_ids = category_names_to_ids(db, &input.categories).await?;
    db.set_project_categories(project_id, &category_ids).await?;
    Ok(project_id)
}

pub async fn delete_project(db: &VaultDb, id: i64) -> Result<ProjectDeleteImpact, String> {
    db.delete_project(id).await
}

pub async fn project_delete_preview(db: &VaultDb, id: i64) -> Result<ProjectDeleteImpact, String> {
    db.preview_delete_project(id).await
}

/// Persists an environment's vars, granting item ownership to `project_id` as
/// a side effect for any referenced item it doesn't already own — but only
/// when that item is global. A local item can never silently gain a second
/// owner through this path; the caller must mark it global first.
pub async fn save_environment(db: &VaultDb, input: EnvironmentInput) -> Result<i64, String> {
    let env_id = db
        .upsert_environment(input.id, input.project_id, &input.name, input.is_default)
        .await?;

    db.set_environment_paths(env_id, &input.paths).await?;

    for v in &input.vars {
        let owners = db.list_owning_projects(v.item_id).await?;
        if owners.contains(&input.project_id) {
            continue;
        }
        if db.is_item_global(v.item_id).await?.unwrap_or(false) {
            db.add_item_owner(v.item_id, input.project_id).await?;
        } else {
            return Err(format!(
                "item {} is not global and not already owned by this project",
                v.item_id
            ));
        }
    }

    let db_vars: Vec<DbEnvironmentVar> = input
        .vars
        .into_iter()
        .map(|v| DbEnvironmentVar { id: 0, environment_id: env_id, key: v.key, item_id: Some(v.item_id), literal: None })
        .collect();

    db.set_environment_vars(env_id, &db_vars).await?;
    Ok(env_id)
}

pub async fn delete_environment(db: &VaultDb, id: i64) -> Result<(), String> {
    db.delete_environment(id).await
}

/// Resolves the environment identified either by numeric `environment_id`,
/// or by a case-insensitive `project` name + `environment` name pair — the
/// same two lookup shapes CLI's `project inject --id` / `--project
/// --environment` already offers against `GET /projects`. Every HTTP
/// endpoint that needs to scope its work to a single project+environment
/// reuses this instead of inventing a second lookup convention.
pub async fn resolve_environment(
    db: &VaultDb,
    environment_id: Option<i64>,
    project: Option<&str>,
    environment: Option<&str>,
) -> Result<Environment, String> {
    if let Some(id) = environment_id {
        let env = get_environment_full(db, id).await?;
        return env.ok_or_else(|| "environment not found".to_string());
    }

    if let (Some(p), Some(e)) = (project, environment) {
        let p_lower = p.to_lowercase();
        let e_lower = e.to_lowercase();
        let projects = list_projects(db).await?;
        return projects
            .into_iter()
            .find(|proj| proj.name.to_lowercase() == p_lower)
            .and_then(|proj| proj.environments.into_iter().find(|env| env.name.to_lowercase() == e_lower))
            .ok_or_else(|| format!("project/environment not found: {p} / {e}"));
    }

    Err("provide environment_id, or both project and environment".to_string())
}

async fn get_environment_full(db: &VaultDb, id: i64) -> Result<Option<Environment>, String> {
    let env = match db.get_environment(id).await? {
        Some(e) => e,
        None => return Ok(None),
    };
    let vars = db.get_environment_vars(id).await?;
    Ok(Some(Environment {
        id: env.id,
        project_id: env.project_id,
        name: env.name,
        is_default: env.is_default,
        paths: env.paths,
        vars: vars
            .into_iter()
            .filter_map(|v| v.item_id.map(|item_id| EnvironmentVar { id: v.id, key: v.key, item_id }))
            .collect(),
        created: env.created,
        updated: env.updated,
    }))
}

/// Resolves this environment's write targets (its configured `paths[]` plus
/// an optional `output_path`/`output_dir`-derived path) and classifies each
/// one via `envfile::inspect`, without decrypting anything or writing a
/// byte. Shared by `inject_environment` (phases 1-2 below) and
/// `inject_environment_preview` (which stops here).
async fn resolve_and_inspect(
    db: &VaultDb,
    environment_id: i64,
    output_path: Option<String>,
    output_dir: Option<String>,
) -> Result<(crate::db::DbEnvironment, Vec<(String, PathOrigin)>, HashMap<String, envfile::Target>), String> {
    let env = db
        .get_environment(environment_id)
        .await?
        .ok_or("environment not found")?;

    let mut resolved: Vec<(String, PathOrigin)> =
        env.paths.iter().cloned().map(|p| (p, PathOrigin::Configured)).collect();

    if let Some(p) = output_path {
        if !resolved.iter().any(|(existing, _)| existing == &p) {
            resolved.push((p, PathOrigin::CallerSupplied));
        }
    } else if resolved.is_empty() {
        if let Some(dir) = output_dir {
            let dir = dir.trim_end_matches(['/', '\\']);
            resolved.push((format!("{dir}/.env.{}", env.name), PathOrigin::CallerSupplied));
        }
    }

    let mut inspected: HashMap<String, envfile::Target> = HashMap::new();
    for (path, _) in &resolved {
        let target = envfile::inspect(Path::new(path)).map_err(|e| format!("inspect {path}: {e}"))?;
        inspected.insert(path.clone(), target);
    }

    Ok((env, resolved, inspected))
}

/// Read-only dry run of `inject_environment`'s path resolution and gate,
/// for the GUI's pre-inject confirm dialog: lists every configured path
/// that is currently `Foreign` (would be reported in `unmanaged_paths` on
/// an actual inject) so the user can be asked before anything is touched.
pub async fn inject_environment_preview(db: &VaultDb, environment_id: i64) -> Result<InjectPreview, String> {
    let (_, resolved, inspected) = resolve_and_inspect(db, environment_id, None, None).await?;

    let paths: Vec<String> = resolved.iter().map(|(p, _)| p.clone()).collect();
    let foreign: Vec<String> = resolved
        .iter()
        .filter(|(p, _)| matches!(inspected.get(p), Some(envfile::Target::Foreign)))
        .map(|(p, _)| p.clone())
        .collect();

    Ok(InjectPreview { paths, foreign })
}

/// Decrypt referenced vault items and write KEY=VALUE pairs into every path
/// configured on this environment, plus `output_path` (if given — added to
/// the configured set, never replacing it) or, when the environment has no
/// configured paths and no `output_path` was given, a single default file
/// named `.env.<environment-name>` inside `output_dir`. Merges with existing
/// file content (existing keys not in the environment are preserved).
/// `written` reports the union of keys touched across ALL paths, not just
/// the first one.
///
/// Gate strictly precedes decryption (issue #8): every resolved path is
/// classified before a single secret is decrypted or a byte is written. A
/// `output_path`/`output_dir`-derived path (`CallerSupplied`) that is
/// `Foreign` is hard-refused unless `overwrite` is `true`. A path the vault
/// owner saved into `environment.paths[]` (`Configured`) is never refused —
/// only reported in `unmanaged_paths` — since gating those would break
/// every pre-existing installation on upgrade (see §4.4 of the issue #8
/// plan); it is written through, taking a `.bak` of its prior contents on
/// this first touch, and self-heals (becomes `Managed`) from then on.
pub async fn inject_environment(
    db: &VaultDb,
    vault_key: &[u8; 32],
    environment_id: i64,
    output_path: Option<String>,
    output_dir: Option<String>,
    overwrite: bool,
) -> Result<InjectResult, String> {
    // Phase 1 — resolve, tracking provenance.
    let (env, resolved, mut cached) =
        resolve_and_inspect(db, environment_id, output_path, output_dir).await?;

    if resolved.is_empty() {
        return Err("environment has no paths configured".into());
    }

    // Phase 2 — gate, strictly before any decryption or write.
    let mut refused: Vec<PathBuf> = Vec::new();
    let mut unmanaged: Vec<String> = Vec::new();
    for (path, origin) in &resolved {
        if matches!(cached.get(path), Some(envfile::Target::Foreign)) {
            match origin {
                PathOrigin::CallerSupplied => refused.push(PathBuf::from(path)),
                PathOrigin::Configured => unmanaged.push(path.clone()),
            }
        }
    }

    if !refused.is_empty() && !overwrite {
        return Err(envfile::refuse_message(&refused));
    }

    let paths: Vec<String> = resolved.iter().map(|(p, _)| p.clone()).collect();
    let vars = db.get_environment_vars(environment_id).await?;

    if vars.is_empty() {
        return Ok(InjectResult { paths, written: vec![], unmanaged_paths: unmanaged, backups: vec![] });
    }

    // Phase 3 — decrypt, only reachable past the gate above.
    let raw_items = db.list_items().await?;
    let mut item_values: HashMap<i64, String> = HashMap::new();
    for (item_id, _, data, _, _) in &raw_items {
        let json = crate::crypto::decrypt(vault_key, data)?;
        let item: crate::vault::VaultItem =
            serde_json::from_slice(&json).map_err(|e| format!("parse item: {e}"))?;
        let value = item.value.or(item.password).or(item.content).unwrap_or_default();
        item_values.insert(*item_id, value);
    }

    // Build key → value map for this environment
    let mut inject_map: HashMap<String, String> = HashMap::new();
    for v in &vars {
        let value = if let Some(iid) = v.item_id {
            item_values.get(&iid).cloned().unwrap_or_default()
        } else {
            v.literal.clone().unwrap_or_default()
        };
        inject_map.insert(v.key.clone(), value);
    }

    // Marker names the project + environment (informational only, never
    // parsed back — see `envfile::marker_line`).
    let project_name = db
        .get_project_name(env.project_id)
        .await?
        .unwrap_or_else(|| "unknown".to_string());
    let marker = envfile::marker_line(&project_name, &env.name);

    // Phase 4 — write. `written` reports the union of keys touched across
    // ALL paths, not just the first one.
    let mut written: HashSet<String> = HashSet::new();
    let mut backups: Vec<String> = Vec::new();
    let mut done: Vec<String> = Vec::new();

    for (path, origin) in &resolved {
        let existing = match cached.remove(path) {
            Some(envfile::Target::Managed(content)) => content,
            _ => String::new(),
        };
        let mut lines: Vec<String> = existing.lines().map(String::from).collect();
        let mut updated_keys: HashSet<String> = HashSet::new();

        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.contains('=') {
                continue;
            }
            let eq_pos = match trimmed.find('=') {
                Some(p) => p,
                None => continue,
            };
            let existing_key = trimmed[..eq_pos].trim().to_string();
            if let Some(new_val) = inject_map.get(&existing_key) {
                *line = format!("{}={}", existing_key, new_val);
                updated_keys.insert(existing_key.clone());
                written.insert(existing_key);
            }
        }

        for (k, v) in &inject_map {
            if !updated_keys.contains(k) {
                lines.push(format!("{}={}", k, v));
                written.insert(k.clone());
            }
        }

        let content = lines.join("\n") + "\n";

        // Configured paths are pre-consented by the vault owner (§4.4) and
        // are written through even if Foreign — never hard-gated. Caller-
        // supplied paths only reach here Absent or Managed, unless the
        // caller explicitly passed `overwrite: true` past the gate above.
        let effective_overwrite = overwrite || *origin == PathOrigin::Configured;
        let opts = envfile::WriteOptions { overwrite: effective_overwrite, mode: envfile::FileMode::Private0600 };

        match envfile::commit(Path::new(path), &content, &marker, &opts) {
            Ok(committed) => {
                if let Some(bak) = committed.backup {
                    backups.push(bak.to_string_lossy().into_owned());
                }
                done.push(path.clone());
            }
            Err(e @ envfile::EnvFileError::BackupExists(_)) | Err(e @ envfile::EnvFileError::TargetExists(_)) => {
                // Keep the bare message so `err.starts_with(BACKUP_EXISTS_PREFIX
                // | TARGET_EXISTS_PREFIX)` still matches in the HTTP handler
                // and this maps to 409, not the generic 500 below.
                // (`TargetExists` should be unreachable here in practice —
                // `effective_overwrite` is always `true` for every path that
                // reaches this loop — kept as a defensive fallback.)
                return Err(e.to_string());
            }
            Err(e) => {
                // Stop-and-report: cross-file atomicity across arbitrary
                // user-chosen locations is not achievable (§4.5) — name
                // the failing path and everything already written.
                let already = if done.is_empty() { "none".to_string() } else { done.join(", ") };
                return Err(format!("write .env ({path}): {e} (already written: {already})"));
            }
        }
    }

    let mut written_keys: Vec<String> = written.into_iter().collect();
    written_keys.sort();

    Ok(InjectResult { paths, written: written_keys, unmanaged_paths: unmanaged, backups })
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn project_list(state: State<'_, SharedState>) -> Result<Vec<Project>, String> {
    let s = state.lock().await;
    list_projects(&s.db).await
}

#[tauri::command]
pub async fn project_save(state: State<'_, SharedState>, project: ProjectInput) -> Result<i64, String> {
    let s = state.lock().await;
    save_project(&s.db, project).await
}

#[tauri::command]
pub async fn project_delete(state: State<'_, SharedState>, id: i64) -> Result<ProjectDeleteImpact, String> {
    let s = state.lock().await;
    delete_project(&s.db, id).await
}

#[tauri::command]
pub async fn project_preview_delete(state: State<'_, SharedState>, id: i64) -> Result<ProjectDeleteImpact, String> {
    let s = state.lock().await;
    project_delete_preview(&s.db, id).await
}

#[tauri::command]
pub async fn environment_save(
    state: State<'_, SharedState>,
    environment: EnvironmentInput,
) -> Result<i64, String> {
    let s = state.lock().await;
    save_environment(&s.db, environment).await
}

#[tauri::command]
pub async fn environment_delete(state: State<'_, SharedState>, id: i64) -> Result<(), String> {
    let s = state.lock().await;
    delete_environment(&s.db, id).await
}

#[tauri::command]
pub async fn environment_inject(
    state: State<'_, SharedState>,
    id: i64,
    overwrite: bool,
) -> Result<InjectResult, String> {
    let s = state.lock().await;
    let key = s.key.as_ref().ok_or("vault is locked")?;
    let vault_key: [u8; 32] = **key;
    inject_environment(&s.db, &vault_key, id, None, None, overwrite).await
}

/// Dry run for the GUI's pre-inject confirm dialog — see
/// `inject_environment_preview`. Only ever inspects `environment.paths[]`:
/// the GUI never supplies a caller `output_path`/`output_dir`, so there is
/// nothing to hard-gate here, only `Foreign` configured paths to surface.
#[tauri::command]
pub async fn environment_inject_preview(
    state: State<'_, SharedState>,
    id: i64,
) -> Result<InjectPreview, String> {
    let s = state.lock().await;
    inject_environment_preview(&s.db, id).await
}

#[tauri::command]
pub async fn project_pick_env_path() -> Result<Option<String>, String> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Select .env file")
            .add_filter("All files", &["*"])
            .add_filter("Env files", &["env"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(result.map(|p| p.to_string_lossy().into_owned()))
}

// ─── Export / Import ──────────────────────────────────────────────────────────
// Exports a whole project (all its environments) as a reusable template. Paths
// are machine-specific and intentionally dropped; item references can't cross
// vaults either, so only literal values survive the round-trip.

#[derive(Serialize, Deserialize)]
pub struct ExportedEnvironmentVar {
    pub key: String,
    pub literal: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportedEnvironment {
    pub name: String,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    pub vars: Vec<ExportedEnvironmentVar>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportedProject {
    pub version: u8,
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub environments: Vec<ExportedEnvironment>,
}

#[tauri::command]
pub async fn project_export(project_id: i64, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().await;
    let project = list_projects(&s.db)
        .await?
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or("project not found")?;

    let exported = ExportedProject {
        version: 1,
        name: project.name.clone(),
        description: project.description.clone(),
        template: project.template.clone(),
        environments: project
            .environments
            .into_iter()
            .map(|e| ExportedEnvironment {
                name: e.name,
                is_default: e.is_default,
                // Every var is a real vault item now, and item values never
                // cross vaults — templates only ever carry KEY names, never
                // resolved values, to avoid leaking secrets into an exported file.
                vars: e
                    .vars
                    .into_iter()
                    .map(|v| ExportedEnvironmentVar { key: v.key, literal: None })
                    .collect(),
            })
            .collect(),
    };

    let json = serde_json::to_vec_pretty(&exported).map_err(|e| e.to_string())?;

    let safe_name = project.name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    let default_name = format!("{}.cryptenv-proj", safe_name);

    let path = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title("Save project template")
            .set_file_name(&default_name)
            .add_filter("CryptEnv Project", &["cryptenv-proj"])
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "cancelled".to_string())?;

    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn project_import() -> Result<ExportedProject, String> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Load project template")
            .add_filter("CryptEnv Project", &["cryptenv-proj"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "cancelled".to_string())?;

    let json = std::fs::read(&path).map_err(|e| e.to_string())?;
    let project: ExportedProject = serde_json::from_slice(&json).map_err(|e| format!("invalid file: {e}"))?;
    Ok(project)
}
