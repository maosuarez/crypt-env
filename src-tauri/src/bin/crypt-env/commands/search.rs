use clap::Args;
use crate::client::{self, CliError, ItemSummary};

#[derive(Args)]
pub struct SearchArgs {
    /// Text to search for
    pub query: String,
}

pub fn run(args: SearchArgs) -> Result<(), CliError> {
    let url = format!("{}/items?search={}", client::API_BASE, client::urlencod(&args.query));
    let resp = client::authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("HTTP error {code}")));
    }

    let items: Vec<ItemSummary> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;

    if items.is_empty() {
        eprintln!("No results found for '{}'.", args.query);
        return Ok(());
    }

    println!("{:<6} {:<16} {:<32} CATEGORIES", "ID", "TYPE", "NAME/TITLE");
    println!("{}", "-".repeat(80));
    for item in &items {
        let display_name = item
            .name
            .as_deref()
            .or(item.title.as_deref())
            .unwrap_or("");
        println!(
            "{:<6} {:<16} {:<32} {}",
            item.id,
            item.item_type,
            display_name,
            item.categories.join(", ")
        );
    }

    Ok(())
}
