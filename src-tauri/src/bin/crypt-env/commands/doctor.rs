use clap::Args;
use crate::client::{API_BASE, CliError, read_token};

#[derive(Args)]
pub struct DoctorArgs {}

pub fn run(_args: DoctorArgs) -> Result<(), CliError> {
    println!("crypt-env doctor — system diagnostics\n");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| CliError::Api(e.to_string()))?;

    match client.get(format!("{API_BASE}/health")).send() {
        Err(e) if e.is_connect() => {
            println!("  [!!] App running        not running — open crypt-env and try again");
            println!();
            return Ok(());
        }
        Err(e) => {
            println!("  [!!] App running        error: {e}");
            println!();
            return Ok(());
        }
        Ok(resp) => {
            let json: serde_json::Value =
                resp.json().map_err(|e| CliError::Api(e.to_string()))?;

            let version = json.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            let vault_locked = json
                .get("vault_locked")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let item_count = json.get("item_count").and_then(|v| v.as_u64());
            let mcp_configured = json
                .get("mcp_token_configured")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            println!("  [OK] App running        http://127.0.0.1:47821  (v{version})");

            if vault_locked {
                println!("  [!!] Vault status       locked");
            } else {
                match item_count {
                    Some(n) => println!("  [OK] Vault status       unlocked  ({n} items)"),
                    None => println!("  [OK] Vault status       unlocked"),
                }
            }

            if mcp_configured {
                println!("  [OK] MCP token          configured in app");
            } else {
                println!("  [--] MCP token          not configured  (Settings → INTEGRATIONS)");
            }
        }
    }

    // CLI session token
    if read_token().is_some() {
        println!("  [OK] CLI session        token cached");
    } else {
        println!("  [--] CLI session        no cached token  (will prompt on next use)");
    }

    // MCP token file on disk
    let mcp_path = std::env::var("APPDATA")
        .map(|d| {
            std::path::PathBuf::from(d)
                .join("com.maosuarez.cryptenv")
                .join("mcp_token")
        })
        .or_else(|_| {
            std::env::var("HOME").map(|d| {
                std::path::PathBuf::from(d)
                    .join(".local")
                    .join("share")
                    .join("com.maosuarez.cryptenv")
                    .join("mcp_token")
            })
        });

    if let Ok(path) = mcp_path {
        if path.exists() {
            println!("  [OK] MCP token file     {}", path.display());
        } else {
            println!("  [--] MCP token file     not found  ({})", path.display());
        }
    }

    println!();
    Ok(())
}
