use clap::Args;
use std::collections::HashSet;
use std::path::PathBuf;
use crate::client::{self, CliError};

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
}

pub fn run(args: AddArgs) -> Result<(), CliError> {
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

    // Fetch all existing item names ONCE for duplicate detection
    let existing: HashSet<String> = client::api_list_all_items()?
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

    for (key, value) in &pairs {
        let body = serde_json::json!({
            "item_type": item_type,
            "name": key,
            "value": value,
        });
        let resp = client::authenticated_post(
            &format!("{}/items", client::API_BASE),
            &body,
        )?;
        if !resp.status().is_success() {
            // Only key name in error — never the value
            eprintln!("Failed to add '{}': HTTP {}", key, resp.status());
        } else {
            eprintln!("Added: {}", key);
        }
    }

    Ok(())
}
