// crypt-env-mcp.rs — Standalone MCP server over stdio.
// Does not import from the project lib — uses only serde, serde_json, reqwest::blocking.
// All user-facing strings are in English to match the CLI.

use std::io::BufRead;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Archivos .env temporales pendientes de limpieza del ciclo anterior.
static TEMP_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

const API_BASE: &str = "http://127.0.0.1:47821";
const MCP_VERSION: &str = "2024-11-05";

// ─── Token ────────────────────────────────────────────────────────────────────

/// Returns the platform-specific path to the MCP token file.
///
/// - Windows: %APPDATA%\com.maosuarez.cryptenv\mcp_token
/// - macOS:   ~/Library/Application Support/com.maosuarez.cryptenv/mcp_token
/// - Linux:   ~/.local/share/com.maosuarez.cryptenv/mcp_token
fn mcp_token_path() -> Result<std::path::PathBuf, String> {
    // Windows: %APPDATA% is always set when the GUI app runs.
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(|d| {
                std::path::PathBuf::from(d)
                    .join("com.maosuarez.cryptenv")
                    .join("mcp_token")
            })
            .map_err(|_| "APPDATA environment variable not set".to_string())
    }

    // macOS: ~/Library/Application Support/
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .map(|d| {
                std::path::PathBuf::from(d)
                    .join("Library")
                    .join("Application Support")
                    .join("com.maosuarez.cryptenv")
                    .join("mcp_token")
            })
            .map_err(|_| "HOME environment variable not set".to_string())
    }

    // Linux: ~/.local/share/
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("HOME")
            .map(|d| {
                std::path::PathBuf::from(d)
                    .join(".local")
                    .join("share")
                    .join("com.maosuarez.cryptenv")
                    .join("mcp_token")
            })
            .map_err(|_| "HOME environment variable not set".to_string())
    }
}

fn read_mcp_token() -> Result<String, String> {
    let path = mcp_token_path()?;
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|_| {
            "MCP token not found. Generate one in crypt-env Settings → INTEGRATIONS.".to_string()
        })
}

// ─── JSON-RPC types ───────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

// ─── Tool call result helpers ─────────────────────────────────────────────────

fn tool_ok(text: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": false
    })
}

fn tool_err(text: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": true
    })
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

fn tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "crypt_env_list_items",
            "description": "List available secrets by name (no values). Use this first to discover which API keys and credentials are stored — so you know what tools and services you can configure before writing a .env.example.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Filter by type: secret, credential, link, command, note" },
                    "category": { "type": "string", "description": "Filter by category name" }
                }
            }
        },
        {
            "name": "crypt_env_get_item",
            "description": "Returns item metadata without the secret value.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Item ID" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "crypt_env_search_items",
            "description": "Search items by name (no secret values exposed). Returns matching items metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search term to match against item names" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "crypt_env_generate_env",
            "description": "Writes a .env file with the real values of the specified secrets. Returns the file path — values never appear in the response.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": { "type": "array", "items": { "type": "string" }, "description": "Names of the secrets to include" }
                },
                "required": ["keys"]
            }
        },
        {
            "name": "crypt_env_inject_env",
            "description": "Injects a secret as an environment variable into the MCP process. Does not return the value.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Secret name" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "crypt_env_add_item",
            "description": "Adds a new item to the vault.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Item type: secret, credential, link, command, note" },
                    "name": { "type": "string" },
                    "value": { "type": "string", "description": "Secret value (for secret/credential types)" },
                    "category": { "type": "string" },
                    "notes": { "type": "string" },
                    "url": { "type": "string" },
                    "username": { "type": "string" }
                },
                "required": ["type", "name"]
            }
        },
        {
            "name": "crypt_env_update_settings",
            "description": "Updates settings: auto_lock_timeout (minutes) and hotkey. Changing master_password is not allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "auto_lock_timeout": { "type": "integer", "description": "Minutes until auto-lock (0 = never)" },
                    "hotkey": { "type": "string", "description": "Global shortcut, e.g. Ctrl+Alt+Z" }
                }
            }
        },
        {
            "name": "crypt_env_fill_env",
            "description": "Fills a .env.example template with real secret values and writes the result directly to output_path on disk. Secret values never appear in the response — use crypt_env_list_items first to discover available keys, then write a .env.example, then call this to produce the final .env the service will read.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "template": { "type": "string", "description": "Content of the .env.example (lines like KEY= or KEY=description)" },
                    "output_path": { "type": "string", "description": "Absolute path where the filled .env should be written, e.g. /home/user/my-project/.env" }
                },
                "required": ["template", "output_path"]
            }
        },
        {
            "name": "crypt_env_doctor",
            "description": "Checks the health of the crypt-env installation: app status, vault lock state, item count, MCP token configuration, and version.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "crypt_env_list_commands",
            "description": "Lists saved commands with name, description, and required placeholders.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "crypt_env_run_command",
            "description": "Returns a command string with {{VAR}} placeholders resolved. Does not execute it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Command name" },
                    "params": {
                        "type": "object",
                        "description": "Map of VAR → value to resolve placeholders",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["name"]
            }
        }
    ])
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

fn vault_get(path: &str, token: &str) -> Result<reqwest::blocking::Response, String> {
    reqwest::blocking::Client::new()
        .get(format!("{API_BASE}{path}"))
        .header("X-Vault-Token", token)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                "Error: crypt-env app is not running. Open the application and try again.".to_string()
            } else {
                e.to_string()
            }
        })
}

fn vault_post(
    path: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<reqwest::blocking::Response, String> {
    reqwest::blocking::Client::new()
        .post(format!("{API_BASE}{path}"))
        .header("X-Vault-Token", token)
        .json(body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                "Error: crypt-env app is not running. Open the application and try again.".to_string()
            } else {
                e.to_string()
            }
        })
}

fn vault_put(
    path: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<reqwest::blocking::Response, String> {
    reqwest::blocking::Client::new()
        .put(format!("{API_BASE}{path}"))
        .header("X-Vault-Token", token)
        .json(body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                "Error: crypt-env app is not running. Open the application and try again.".to_string()
            } else {
                e.to_string()
            }
        })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn urlencod(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push('+'),
            c => {
                for byte in c.to_string().as_bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    out
}

/// Valida que el nombre de variable de entorno sea seguro: ^[A-Z][A-Z0-9_]*$
/// y bloquea variables críticas del sistema.
fn is_safe_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    if !chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return false;
    }
    const BLOCKED: &[&str] = &[
        "PATH",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
        "PYTHONPATH",
        "NODE_OPTIONS",
        "RUBYOPT",
    ];
    if BLOCKED.contains(&key) {
        return false;
    }
    if key.starts_with("LD_") {
        return false;
    }
    true
}

fn cleanup_prev_temp_files() {
    if let Ok(mut files) = TEMP_FILES.lock() {
        files.retain(|p| std::fs::remove_file(p).is_err());
    }
}

fn track_temp_file(path: PathBuf) {
    if let Ok(mut files) = TEMP_FILES.lock() {
        files.push(path);
    }
}

// ─── Tool implementations ─────────────────────────────────────────────────────

fn tool_list_items(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let mut url = "/items".to_string();
    let mut sep = '?';
    if let Some(t) = args.get("type").and_then(|v| v.as_str()) {
        url.push_str(&format!("{}type={}", sep, t));
        sep = '&';
    }
    if let Some(cat) = args.get("category").and_then(|v| v.as_str()) {
        url.push_str(&format!("{}category={}", sep, cat));
    }

    let resp = match vault_get(&url, token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if resp.status().as_u16() == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading response: {e}")),
    };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_search_items(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return tool_err("required parameter: 'query'"),
    };

    let url = format!("/items?search={}", urlencod(&query));
    let resp = match vault_get(&url, token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if resp.status().as_u16() == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading response: {e}")),
    };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_get_item(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let id = match args.get("id").and_then(|v| v.as_i64()) {
        Some(i) => i,
        None => return tool_err("required parameter: 'id'"),
    };

    let resp = match vault_get(&format!("/items/{id}"), token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let status = resp.status().as_u16();
    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading response: {e}")),
    };

    if status == 404 {
        return tool_err("item not found");
    }
    if status == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_generate_env(args: &serde_json::Value, token: &str) -> serde_json::Value {
    // Limpiar archivos .env del ciclo anterior antes de crear uno nuevo
    cleanup_prev_temp_files();

    let keys: Vec<String> = match args.get("keys").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => return tool_err("required parameter: 'keys' (array of strings)"),
    };

    let mut env_lines: Vec<String> = Vec::new();
    let mut n_found = 0usize;

    for key in &keys {
        if !is_safe_env_key(key) {
            env_lines.push(format!("# {key}: invalid or blocked name, skipped"));
            continue;
        }

        let search_url = format!("/items?search={}", urlencod(key));
        let items_resp = match vault_get(&search_url, token) {
            Ok(r) => r,
            Err(e) => return tool_err(e),
        };

        if items_resp.status().as_u16() == 403 {
            return tool_err("vault_locked: unlock the vault first");
        }

        let items_text = match items_resp.text() {
            Ok(t) => t,
            Err(e) => return tool_err(format!("error reading items: {e}")),
        };

        let items_val: serde_json::Value = match serde_json::from_str(&items_text) {
            Ok(v) => v,
            Err(_) => {
                env_lines.push(format!("# {key}: error parsing response"));
                continue;
            }
        };

        let key_lower = key.to_lowercase();
        let found_id = items_val
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|item| {
                    item.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.to_lowercase() == key_lower)
                        .unwrap_or(false)
                })
            })
            .and_then(|item| item.get("id").and_then(|v| v.as_i64()));

        let item_id = match found_id {
            Some(id) => id,
            None => {
                env_lines.push(format!("# {key}: not found in vault"));
                continue;
            }
        };

        let reveal_resp = match vault_post(
            &format!("/items/{item_id}/reveal"),
            token,
            &serde_json::json!({"confirm": true}),
        ) {
            Ok(r) => r,
            Err(e) => return tool_err(e),
        };

        let reveal_text = match reveal_resp.text() {
            Ok(t) => t,
            Err(e) => return tool_err(format!("error reading reveal: {e}")),
        };

        let reveal_val: serde_json::Value = match serde_json::from_str(&reveal_text) {
            Ok(v) => v,
            Err(_) => {
                env_lines.push(format!("# {key}: error parsing value"));
                continue;
            }
        };

        let value = reveal_val
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        env_lines.push(format!("{key}={value}"));
        n_found += 1;
    }

    let content = env_lines.join("\n");
    let filename = format!("crypt_env_{}.env", random_hex(8));
    let path = std::env::temp_dir().join(&filename);

    if let Err(e) = std::fs::write(&path, &content) {
        return tool_err(format!("error writing .env file: {e}"));
    }

    track_temp_file(path.clone());

    let path_str = path.to_string_lossy().to_string();
    tool_ok(
        serde_json::to_string_pretty(&serde_json::json!({
            "path": path_str,
            "count": n_found,
            "note": "This file contains secrets in plaintext. Load it and delete it immediately."
        }))
        .unwrap_or_default(),
    )
}

fn tool_inject_env(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return tool_err("required parameter: 'key'"),
    };

    if !is_safe_env_key(&key) {
        return tool_err(format!(
            "invalid or blocked variable name: '{key}'. \
             Must match [A-Z][A-Z0-9_]* and cannot be a critical system variable."
        ));
    }

    let search_url = format!("/items?search={}", urlencod(&key));
    let items_resp = match vault_get(&search_url, token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if items_resp.status().as_u16() == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    let items_text = match items_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading items: {e}")),
    };

    let items_val: serde_json::Value = match serde_json::from_str(&items_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parsing item list"),
    };

    let key_lower = key.to_lowercase();
    let found_id = items_val
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|item| {
                item.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.to_lowercase() == key_lower)
                    .unwrap_or(false)
            })
        })
        .and_then(|item| item.get("id").and_then(|v| v.as_i64()));

    let item_id = match found_id {
        Some(id) => id,
        None => return tool_err(format!("secret '{key}' not found in vault")),
    };

    let reveal_resp = match vault_post(
        &format!("/items/{item_id}/reveal"),
        token,
        &serde_json::json!({"confirm": true}),
    ) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let reveal_text = match reveal_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading reveal: {e}")),
    };

    let reveal_val: serde_json::Value = match serde_json::from_str(&reveal_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parsing revealed value"),
    };

    let value = match reveal_val.get("value").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return tool_err("item has no secret value"),
    };

    // Inyectar como variable de entorno
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var(&key, &value);
    }

    tool_ok(
        serde_json::to_string_pretty(&serde_json::json!({
            "injected": true,
            "name": key
        }))
        .unwrap_or_default(),
    )
}

fn tool_add_item(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let item_type = match args.get("type").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return tool_err("required parameter: 'type'"),
    };
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return tool_err("required parameter: 'name'"),
    };

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let categories = if let Some(cat) = args.get("category").and_then(|v| v.as_str()) {
        serde_json::json!([cat])
    } else {
        serde_json::json!([])
    };

    let mut body = serde_json::json!({
        "id": 0,
        "type": item_type,
        "name": name,
        "categories": categories,
        "created": now_ts
    });

    // Campos opcionales
    for field in &["value", "notes", "url", "username"] {
        if let Some(v) = args.get(field).and_then(|v| v.as_str()) {
            body[field] = serde_json::json!(v);
        }
    }

    let resp = match vault_post("/items", token, &body) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let status = resp.status().as_u16();
    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo respuesta: {e}")),
    };

    if status == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }
    if status >= 400 {
        return tool_err(format!("error creating item (HTTP {status}): {text}"));
    }

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_update_settings(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let resp = match vault_put("/settings", token, args) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let status = resp.status().as_u16();
    if status == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }
    if status >= 400 {
        let text = resp.text().unwrap_or_default();
        return tool_err(format!("error updating settings (HTTP {status}): {text}"));
    }

    tool_ok(
        serde_json::to_string_pretty(&serde_json::json!({ "ok": true })).unwrap_or_default(),
    )
}

fn tool_fill_env(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let template = match args.get("template").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return tool_err("required parameter: 'template'"),
    };
    let output_path = match args.get("output_path").and_then(|v| v.as_str()) {
        Some(p) => std::path::PathBuf::from(p),
        None => return tool_err("required parameter: 'output_path'"),
    };

    let resp = match vault_post(
        "/fill",
        token,
        &serde_json::json!({ "template": template, "output_path": output_path.to_string_lossy() }),
    ) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let status = resp.status().as_u16();
    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading response: {e}")),
    };

    if status == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }
    if status >= 400 {
        return tool_err(format!("fill failed (HTTP {status}): {text}"));
    }

    // The API already wrote the file to output_path — just surface the stats.
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_doctor(_args: &serde_json::Value, _token: &str) -> serde_json::Value {
    let resp = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .and_then(|c| c.get(format!("{API_BASE}/health")).send())
    {
        Ok(r) => r,
        Err(e) => {
            return tool_ok(
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "not_running",
                    "error": "crypt-env app is not running. Open the application and try again.",
                    "detail": e.to_string()
                }))
                .unwrap_or_default(),
            )
        }
    };

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading health response: {e}")),
    };

    let mut health: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return tool_ok(text),
    };

    // Add MCP token file status
    let mcp_token_file_ok = mcp_token_path()
        .map(|p| p.exists())
        .unwrap_or(false);

    health["mcp_server"] = serde_json::json!("running");
    health["mcp_token_file_present"] = serde_json::json!(mcp_token_file_ok);

    tool_ok(serde_json::to_string_pretty(&health).unwrap_or(text))
}

fn tool_list_commands(token: &str) -> serde_json::Value {
    let resp = match vault_get("/commands", token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if resp.status().as_u16() == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading response: {e}")),
    };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_run_command(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let cmd_name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return tool_err("required parameter: 'name'"),
    };

    let list_resp = match vault_get("/commands", token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if list_resp.status().as_u16() == 403 {
        return tool_err("vault_locked: unlock the vault first");
    }

    let list_text = match list_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading commands: {e}")),
    };

    let list_val: serde_json::Value = match serde_json::from_str(&list_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parsing command list"),
    };

    let name_lower = cmd_name.to_lowercase();
    let found = list_val
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|cmd| {
                cmd.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.to_lowercase() == name_lower)
                    .unwrap_or(false)
            })
        })
        .and_then(|cmd| cmd.get("id").and_then(|v| v.as_i64()));

    let cmd_id = match found {
        Some(id) => id,
        None => return tool_err(format!("command '{cmd_name}' not found")),
    };

    let detail_resp = match vault_get(&format!("/commands/{cmd_id}"), token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let detail_text = match detail_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error reading command detail: {e}")),
    };

    let detail_val: serde_json::Value = match serde_json::from_str(&detail_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parsing command detail"),
    };

    let template = match detail_val.get("command").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return tool_err("command has no template"),
    };

    // Sustituir placeholders {{VAR}} con valores de params
    let mut resolved = template.clone();
    if let Some(params_map) = args.get("params").and_then(|v| v.as_object()) {
        for (var, val) in params_map {
            if let Some(val_str) = val.as_str() {
                let placeholder = format!("{{{{{}}}}}", var);
                resolved = resolved.replace(&placeholder, val_str);
            }
        }
    }

    tool_ok(resolved)
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

fn dispatch(
    method: &str,
    params: &serde_json::Value,
    token: &str,
) -> Result<serde_json::Value, RpcError> {
    match method {
        "initialize" => Ok(serde_json::json!({
            "protocolVersion": MCP_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "crypt-env", "version": "0.1.0" }
        })),
        "ping" => Ok(serde_json::json!({})),
        "tools/list" => Ok(serde_json::json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));
            Ok(handle_tool_call(name, &args, token))
        }
        _ => Err(RpcError {
            code: -32601,
            message: format!("Method not found: {method}"),
        }),
    }
}

fn handle_tool_call(name: &str, args: &serde_json::Value, token: &str) -> serde_json::Value {
    match name {
        "crypt_env_list_items" => tool_list_items(args, token),
        "crypt_env_get_item" => tool_get_item(args, token),
        "crypt_env_search_items" => tool_search_items(args, token),
        "crypt_env_fill_env" => tool_fill_env(args, token),
        "crypt_env_generate_env" => tool_generate_env(args, token),
        "crypt_env_inject_env" => tool_inject_env(args, token),
        "crypt_env_add_item" => tool_add_item(args, token),
        "crypt_env_update_settings" => tool_update_settings(args, token),
        "crypt_env_doctor" => tool_doctor(args, token),
        "crypt_env_list_commands" => tool_list_commands(token),
        "crypt_env_run_command" => tool_run_command(args, token),
        _ => tool_err(format!("unknown tool: {name}")),
    }
}

// ─── I/O ──────────────────────────────────────────────────────────────────────

fn writeln_json(stdout: &std::io::Stdout, val: &impl Serialize) {
    let mut lock = stdout.lock();
    let _ = serde_json::to_writer(&mut lock, val);
    let _ = writeln!(lock);
    let _ = lock.flush();
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let token = match read_mcp_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[crypt-env-mcp] {e}");
            std::process::exit(1);
        }
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => l,
            Err(_) => break,
        };

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = RpcResponse {
                    jsonrpc: "2.0",
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(RpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                    }),
                };
                writeln_json(&stdout, &resp);
                continue;
            }
        };

        // Notificaciones (sin id) — no requieren respuesta
        if req.id.is_none() {
            continue;
        }

        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
        let result = dispatch(&req.method, &req.params, &token);
        let resp = match result {
            Ok(r) => RpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(r),
                error: None,
            },
            Err(e) => RpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(e),
            },
        };
        writeln_json(&stdout, &resp);
    }
}
