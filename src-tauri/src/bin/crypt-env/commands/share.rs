use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::client::{authenticated_get, authenticated_post, CliError, API_BASE};

// ─── CLI argument structs ─────────────────────────────────────────────────────

#[derive(Args)]
pub struct ShareArgs {
    #[command(subcommand)]
    pub cmd: ShareCmd,
}

#[derive(Subcommand)]
pub enum ShareCmd {
    /// Share selected vault items over the local network (sender)
    Send {
        /// Item IDs to share
        #[arg(required = true, num_args = 1..)]
        items: Vec<i64>,
    },
    /// Receive shared items over the local network (receiver)
    Receive,
    /// Export selected items as an encrypted .vault file
    Export {
        /// Item IDs to export
        #[arg(required = true, num_args = 1..)]
        items: Vec<i64>,
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Import items from an encrypted .vault file
    Import {
        /// Path to the .vault package file
        #[arg(short, long)]
        file: PathBuf,
    },
}

pub fn run(args: ShareArgs) -> Result<(), CliError> {
    match args.cmd {
        ShareCmd::Send { items } => run_send(items),
        ShareCmd::Receive => run_receive(),
        ShareCmd::Export { items, output } => run_export(items, output),
        ShareCmd::Import { file } => run_import(file),
    }
}

// ─── Send ─────────────────────────────────────────────────────────────────────

fn run_send(items: Vec<i64>) -> Result<(), CliError> {
    // 1. Start listen session
    let body = serde_json::json!({ "items": items });
    let resp = authenticated_post(&format!("{API_BASE}/share/listen"), &body)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("listen failed: {text}")));
    }

    let listen_data: serde_json::Value =
        resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let pairing_code = listen_data["pairing_code"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    println!();
    println!("Share this code with your teammate: {} (expires in 5 min)", pairing_code);
    println!("Waiting for peer to connect...");

    // 2. Poll until AwaitingFingerprint
    let fingerprint = poll_until_fingerprint()?;

    println!();
    println!("Fingerprint: {} — confirm your teammate sees the same", fingerprint);
    println!();

    // 3. Prompt user for confirmation
    let confirmed = prompt_confirm("Confirm fingerprint? [y/N]: ")?;

    // 4. POST /share/confirm
    let confirm_body = serde_json::json!({ "confirmed": confirmed });
    let confirm_resp =
        authenticated_post(&format!("{API_BASE}/share/confirm"), &confirm_body)?;
    if !confirm_resp.status().is_success() {
        let text = confirm_resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("confirm failed: {text}")));
    }

    if !confirmed {
        println!("Cancelled.");
        return Ok(());
    }

    println!("Fingerprint confirmed. Sending items...");

    // 5. Poll until Done or Failed
    poll_until_terminal("send")?;

    println!("Items shared successfully.");
    Ok(())
}

// ─── Receive ──────────────────────────────────────────────────────────────────

fn run_receive() -> Result<(), CliError> {
    // 1. Prompt for pairing code
    let pairing_code = {
        use std::io::{self, Write};
        print!("Pairing code: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(CliError::Io)?;
        input.trim().to_string()
    };

    if pairing_code.is_empty() {
        return Err(CliError::Api("pairing code is required".into()));
    }

    // 2. POST /share/connect
    let body = serde_json::json!({ "pairing_code": pairing_code });
    let resp = authenticated_post(&format!("{API_BASE}/share/connect"), &body)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("connect failed: {text}")));
    }

    let connect_data: serde_json::Value =
        resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let fingerprint = connect_data["fingerprint"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    println!();
    println!("Fingerprint: {} — confirm your teammate sees the same", fingerprint);
    println!();

    // 3. Prompt for confirmation
    let confirmed = prompt_confirm("Confirm fingerprint? [y/N]: ")?;

    // 4. POST /share/confirm
    let confirm_body = serde_json::json!({ "confirmed": confirmed });
    let confirm_resp =
        authenticated_post(&format!("{API_BASE}/share/confirm"), &confirm_body)?;
    if !confirm_resp.status().is_success() {
        let text = confirm_resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("confirm failed: {text}")));
    }

    if !confirmed {
        println!("Cancelled.");
        return Ok(());
    }

    println!("Receiving items...");

    // 5. Poll until Done or Failed
    let names = poll_until_terminal_with_names()?;

    println!("Imported {} item(s):", names.len());
    for name in &names {
        println!("  - {}", name);
    }
    Ok(())
}

// ─── Export ───────────────────────────────────────────────────────────────────

fn run_export(items: Vec<i64>, output: PathBuf) -> Result<(), CliError> {
    let output_str = output.to_string_lossy().to_string();
    let body = serde_json::json!({
        "items": items,
        "output_path": output_str,
    });

    let resp = authenticated_post(&format!("{API_BASE}/share/export"), &body)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("export failed: {text}")));
    }

    let data: serde_json::Value = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let passphrase = data["passphrase"].as_str().unwrap_or("").to_string();
    let path = data["path"].as_str().unwrap_or(&output_str);

    println!();
    println!("╔════════════════════════════════════════════════════╗");
    println!("║  PASSPHRASE (save this now, will not be shown again) ║");
    println!("╠════════════════════════════════════════════════════╣");
    println!("║  {}  ║", passphrase);
    println!("╚════════════════════════════════════════════════════╝");
    println!();
    println!("Package written to: {}", path);
    Ok(())
}

// ─── Import ───────────────────────────────────────────────────────────────────

fn run_import(file: PathBuf) -> Result<(), CliError> {
    let passphrase =
        rpassword::prompt_password("Package passphrase: ").map_err(CliError::Io)?;

    let body = serde_json::json!({
        "path": file.to_string_lossy(),
        "passphrase": passphrase,
    });

    let resp = authenticated_post(&format!("{API_BASE}/share/import"), &body)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(CliError::Api(format!("import failed: {text}")));
    }

    let data: serde_json::Value = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let count = data["imported"].as_u64().unwrap_or(0);
    let names: Vec<String> = data["item_names"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    println!("Imported {} item(s):", count);
    for name in &names {
        println!("  - {}", name);
    }
    Ok(())
}

// ─── Polling helpers ──────────────────────────────────────────────────────────

/// Poll GET /share/status until state is "awaiting_fingerprint", then return fingerprint.
fn poll_until_fingerprint() -> Result<String, CliError> {
    let max_polls = 300; // 5 minutes at 1s intervals
    for _ in 0..max_polls {
        std::thread::sleep(std::time::Duration::from_secs(1));

        let resp = authenticated_get(&format!("{API_BASE}/share/status"))?;
        if !resp.status().is_success() {
            continue;
        }

        let data: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let state = data["state"].as_str().unwrap_or("");
        match state {
            "awaiting_fingerprint" => {
                let fp = data["fingerprint"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                return Ok(fp);
            }
            "failed" | "cancelled" => {
                return Err(CliError::Api(format!("session ended: {state}")));
            }
            _ => continue,
        }
    }
    Err(CliError::Api("timed out waiting for peer to connect".into()))
}

/// Poll GET /share/status until terminal state (done/failed/cancelled).
fn poll_until_terminal(context: &str) -> Result<(), CliError> {
    let max_polls = 600;
    for _ in 0..max_polls {
        std::thread::sleep(std::time::Duration::from_secs(1));

        let resp = authenticated_get(&format!("{API_BASE}/share/status"))?;
        if !resp.status().is_success() {
            continue;
        }

        let data: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let state = data["state"].as_str().unwrap_or("");
        match state {
            "done" => return Ok(()),
            s if s.starts_with("failed") => {
                return Err(CliError::Api(format!("{context} failed: {s}")));
            }
            "cancelled" => {
                return Err(CliError::Api(format!("{context} was cancelled")));
            }
            _ => continue,
        }
    }
    Err(CliError::Api(format!("{context} timed out")))
}

/// Poll until done and return received item names (receiver side).
fn poll_until_terminal_with_names() -> Result<Vec<String>, CliError> {
    let max_polls = 600;
    for _ in 0..max_polls {
        std::thread::sleep(std::time::Duration::from_secs(1));

        let resp = authenticated_get(&format!("{API_BASE}/share/status"))?;
        if !resp.status().is_success() {
            continue;
        }

        let data: serde_json::Value = match resp.json() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let state = data["state"].as_str().unwrap_or("");
        match state {
            "done" => {
                let names: Vec<String> = data["received_names"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                return Ok(names);
            }
            s if s.starts_with("failed") => {
                return Err(CliError::Api(format!("receive failed: {s}")));
            }
            "cancelled" => {
                return Err(CliError::Api("receive was cancelled".into()));
            }
            _ => continue,
        }
    }
    Err(CliError::Api("receive timed out".into()))
}

// ─── Prompt helper ────────────────────────────────────────────────────────────

fn prompt_confirm(prompt: &str) -> Result<bool, CliError> {
    use std::io::{self, Write};
    print!("{}", prompt);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(CliError::Io)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}
