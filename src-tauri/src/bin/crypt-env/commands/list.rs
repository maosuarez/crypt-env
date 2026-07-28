use clap::Args;
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use regex::Regex;
use crate::client::{self, CliError, CommandDetail};
use crate::commands::scope;

#[derive(Args)]
pub struct ListArgs {
    /// Filter by regex (matches name or description)
    #[arg(long, short = 's')]
    pub search: Option<String>,

    /// Item type to list (default: commands)
    #[arg(long, default_value = "command")]
    pub r#type: String,

    /// Project name (defaults to crypt-env.json or the cwd folder name)
    #[arg(long)]
    pub project: Option<String>,

    /// Environment name (defaults to crypt-env.json or the project's default environment)
    #[arg(long = "env")]
    pub env: Option<String>,
}

pub fn run(args: ListArgs) -> Result<(), CliError> {
    let resolved_scope = scope::resolve(args.project.as_deref(), args.env.as_deref(), false)?;
    let url = resolved_scope.append_query(&format!("{}/commands", client::API_BASE));
    let resp = client::authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        return Err(CliError::Api(format!("HTTP {}", resp.status())));
    }

    let mut commands: Vec<CommandDetail> = resp
        .json()
        .map_err(|e| CliError::Api(e.to_string()))?;

    // Apply regex filter if provided
    if let Some(pattern) = &args.search {
        let re = Regex::new(pattern)
            .map_err(|e| CliError::Api(format!("Invalid regex: {}", e)))?;
        commands.retain(|c| {
            re.is_match(&c.name)
                || c.description
                    .as_deref()
                    .map(|d| re.is_match(d))
                    .unwrap_or(false)
        });
    }

    if commands.is_empty() {
        eprintln!("No commands found.");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Name", "Description", "Shell", "Placeholders"]);

    for cmd in &commands {
        let placeholders = cmd.placeholders.join(", ");
        table.add_row(vec![
            cmd.name.as_str(),
            cmd.description.as_deref().unwrap_or(""),
            cmd.shell.as_deref().unwrap_or(""),
            &placeholders,
        ]);
    }

    println!("{table}");
    Ok(())
}
