use clap::Args;
use std::collections::HashSet;
use std::path::PathBuf;
use crate::client::{self, CliError};
use crate::commands::scope;

#[derive(Args)]
pub struct AddArgs {
    /// KEY=value literal OR $VARNAME to read from system environment
    pub input: Option<String>,

    /// Load from a .env file (default: ./.env in current dir)
    #[arg(long, value_name = "PATH")]
    pub file: Option<Option<PathBuf>>,

    /// Store as credential type instead of secret
    #[arg(long)]
    pub credential: bool,

    /// Store as note type
    #[arg(long)]
    pub note: bool,

    /// Override the stored key name
    #[arg(long)]
    pub name: Option<String>,

    /// Skip confirmation when overwriting existing keys
    #[arg(long)]
    pub force: bool,

    /// Project to add this variable to (defaults to crypt-env.json or the cwd folder name)
    #[arg(long)]
    pub project: Option<String>,

    /// Environment to add this variable to (defaults to crypt-env.json or the project's default environment)
    #[arg(long = "env")]
    pub env: Option<String>,
}

pub fn run(args: AddArgs) -> Result<(), CliError> {
    // `add` (including its `--file` bulk-import mode) is the one command
    // allowed to silently create a not-yet-existing project — see
    // scope::resolve's docs for why every other command is not.
    let resolved_scope = scope::resolve(args.project.as_deref(), args.env.as_deref(), true)?;

    let mut pairs: Vec<(String, String)> = Vec::new();

    let item_type = if args.credential {
        "credential"
    } else if args.note {
        "note"
    } else {
        "secret"
    };

    if let Some(ref input_str) = args.input {
        if input_str.starts_with('$') {
            // Read from system environment variable
            let varname = &input_str[1..];
            let value = std::env::var(varname).map_err(|_| {
                CliError::NotFound(format!("system env var ${}", varname))
            })?;
            let key = args.name.clone().unwrap_or_else(|| varname.to_string());
            pairs.push((key, value));
        } else if let Some(eq_pos) = input_str.find('=') {
            // KEY=value literal
            let key = input_str[..eq_pos].to_string();
            let value = input_str[eq_pos + 1..].to_string();
            let key = args.name.clone().unwrap_or(key);
            pairs.push((key, value));
        } else {
            return Err(CliError::Api(
                "Input must be KEY=value, $VARNAME, or use --file".to_string(),
            ));
        }
    } else if args.file.is_some() {
        // Load from .env file
        let path = match args.file.flatten() {
            Some(p) => p,
            None => {
                let cwd = std::env::current_dir().map_err(CliError::Io)?;
                let candidate = cwd.join(".env");
                if !candidate.exists() {
                    return Err(CliError::NotFound(".env in current directory".to_string()));
                }
                candidate
            }
        };

        for item in dotenvy::from_path_iter(&path)
            .map_err(|e| CliError::Api(e.to_string()))?
        {
            let (key, value) = item.map_err(|e| CliError::Api(e.to_string()))?;
            pairs.push((key, value));
        }
    } else {
        return Err(CliError::Api(
            "Provide KEY=value, $VARNAME, or --file".to_string(),
        ));
    }

    if pairs.is_empty() {
        eprintln!("No entries to add.");
        return Ok(());
    }

    // Fetch existing item names ONCE, scoped to the resolved project +
    // environment, for duplicate detection.
    let existing: HashSet<String> = client::api_list_items(&resolved_scope.to_query_string())?
        .into_iter()
        .filter_map(|item| item.name)
        .collect();

    let conflict_keys: Vec<String> = pairs
        .iter()
        .filter(|(k, _)| existing.contains(k))
        .map(|(k, _)| k.clone())
        .collect();

    if !conflict_keys.is_empty() && !args.force {
        // Show only key names — never values
        eprintln!("The following keys already exist: {}", conflict_keys.join(", "));
        if !crate::prompts::confirm("Update all conflicting keys?") {
            let conflict_set: HashSet<&str> =
                conflict_keys.iter().map(|s| s.as_str()).collect();
            pairs.retain(|(k, _)| !conflict_set.contains(k.as_str()));
        }
    }

    let items_url = resolved_scope.append_query(&format!("{}/items", client::API_BASE));

    for (key, value) in &pairs {
        let body = serde_json::json!({
            "id": 0,
            "type": item_type,
            "name": key,
            "value": value,
            "categories": [],
            "created": "",
            "key": key,
        });
        let resp = client::authenticated_post(&items_url, &body)?;
        if !resp.status().is_success() {
            // Only key name in error — never the value
            eprintln!("Failed to add '{}': HTTP {}", key, resp.status());
        } else {
            eprintln!("Added: {} ({} / {})", key, resolved_scope.project, resolved_scope.environment);
        }
    }

    Ok(())
}
