use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub const API_BASE: &str = "https://127.0.0.1:47821";

// ─── Error types ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CliError {
    Api(String),
    Io(std::io::Error),
    ConnectionRefused,
    Unauthorized,
    NotFound(String),
    VaultLocked,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Api(msg) => write!(f, "API error: {msg}"),
            CliError::Io(e) => write!(f, "I/O error: {e}"),
            CliError::ConnectionRefused => write!(
                f,
                "Error: vault is not running. Open the application and try again."
            ),
            CliError::Unauthorized => write!(f, "Error: unauthorized (invalid token)"),
            CliError::NotFound(name) => write!(f, "Error: '{}' not found in vault", name),
            CliError::VaultLocked => write!(f, "Error: vault is locked"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

// ─── API response types ───────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
pub struct UnlockResponse {
    pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct ApiError {
    pub error: String,
}

#[derive(Deserialize, Debug)]
pub struct ItemSummary {
    pub id: i64,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct CommandDetail {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub placeholders: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RevealResponse {
    value: String,
}

// ─── Session token ────────────────────────────────────────────────────────────

fn token_path() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("com.maosuarez.cryptenv").join(".cli_token"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.maosuarez.cryptenv")
                .join(".cli_token"),
        )
    } else {
        None
    }
}

pub fn read_token() -> Option<String> {
    let path = token_path()?;
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn save_token(token: &str) {
    if let Some(path) = token_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, token);
    }
}

pub fn clear_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(&path);
    }
}

// ─── HTTP client ──────────────────────────────────────────────────────────────

/// Returns the path where the Tauri app stores its TLS certificate.
/// Mirrors the path used by `src-tauri/src/tls/mod.rs`.
fn tls_cert_path() -> Option<PathBuf> {
    // Windows: %APPDATA%\com.maosuarez.cryptenv\tls\cert.pem
    // Linux/macOS: $HOME/.local/share/com.maosuarez.cryptenv/tls/cert.pem (or XDG)
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(
            PathBuf::from(appdata)
                .join("com.maosuarez.cryptenv")
                .join("tls")
                .join("cert.pem"),
        );
    }
    // Fallback for Linux/macOS (XDG data home).
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return Some(
            PathBuf::from(xdg)
                .join("com.maosuarez.cryptenv")
                .join("tls")
                .join("cert.pem"),
        );
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.maosuarez.cryptenv")
                .join("tls")
                .join("cert.pem"),
        );
    }
    None
}

/// Build a `reqwest` blocking client that trusts the vault's self-signed cert.
///
/// The cert is loaded from disk at the same path where the Tauri app stores it.
/// If the cert file cannot be read (e.g. the app hasn't run yet), the function
/// returns a connection-refused error via the `CliError::ConnectionRefused`
/// variant so the caller shows a clear "start the app first" message.
pub fn http_client() -> reqwest::blocking::Client {
    build_http_client().unwrap_or_else(|_| {
        // Fallback: plain client — it will fail on TLS handshake, which will
        // surface as a connection error with an appropriate message to the user.
        reqwest::blocking::Client::new()
    })
}

fn build_http_client() -> Result<reqwest::blocking::Client, Box<dyn std::error::Error>> {
    let cert_path = tls_cert_path()
        .ok_or("cannot determine cert path")?;

    let pem_bytes = std::fs::read(&cert_path)?;
    let cert = reqwest::Certificate::from_pem(&pem_bytes)?;

    let client = reqwest::blocking::ClientBuilder::new()
        .add_root_certificate(cert)
        // Do NOT use danger_accept_invalid_certs — we load the actual cert.
        .build()?;

    Ok(client)
}

/// POST /unlock — returns session token.
pub fn api_unlock(password: &str) -> Result<String, CliError> {
    let client = http_client();
    let body = serde_json::json!({ "master_password": password });

    let resp = client
        .post(format!("{API_BASE}/unlock"))
        .json(&body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status().is_success() {
        let data: UnlockResponse = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
        Ok(data.token)
    } else {
        let err: ApiError = resp.json().unwrap_or(ApiError { error: "unknown error".into() });
        Err(CliError::Api(err.error))
    }
}

/// Returns a valid token: uses saved one or prompts for password.
pub fn get_auth_token() -> Result<String, CliError> {
    if let Some(token) = read_token() {
        return Ok(token);
    }

    let password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
    let token = api_unlock(&password)?;
    save_token(&token);
    Ok(token)
}

/// Authenticated GET. Handles 401 by clearing token and retrying once.
pub fn authenticated_get(url: &str) -> Result<reqwest::blocking::Response, CliError> {
    let token = get_auth_token()?;
    let client = http_client();

    let resp = client
        .get(url)
        .header("X-Vault-Token", &token)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        clear_token();
        let new_password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
        let new_token = api_unlock(&new_password)?;
        save_token(&new_token);

        let resp2 = client
            .get(url)
            .header("X-Vault-Token", &new_token)
            .send()
            .map_err(|e| {
                if e.is_connect() {
                    CliError::ConnectionRefused
                } else {
                    CliError::Api(e.to_string())
                }
            })?;

        return Ok(resp2);
    }

    Ok(resp)
}

/// Authenticated POST with JSON body. Handles 401 with one retry.
pub fn authenticated_post(
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::blocking::Response, CliError> {
    let token = get_auth_token()?;
    let client = http_client();

    let resp = client
        .post(url)
        .header("X-Vault-Token", &token)
        .json(body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        clear_token();
        let new_password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
        let new_token = api_unlock(&new_password)?;
        save_token(&new_token);

        let resp2 = client
            .post(url)
            .header("X-Vault-Token", &new_token)
            .json(body)
            .send()
            .map_err(|e| {
                if e.is_connect() {
                    CliError::ConnectionRefused
                } else {
                    CliError::Api(e.to_string())
                }
            })?;

        return Ok(resp2);
    }

    Ok(resp)
}

/// Authenticated PUT with JSON body. Handles 401 with one retry.
#[allow(dead_code)]
pub fn authenticated_put(
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::blocking::Response, CliError> {
    let token = get_auth_token()?;
    let client = http_client();

    let resp = client
        .put(url)
        .header("X-Vault-Token", &token)
        .json(body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        clear_token();
        let new_password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
        let new_token = api_unlock(&new_password)?;
        save_token(&new_token);

        let resp2 = client
            .put(url)
            .header("X-Vault-Token", &new_token)
            .json(body)
            .send()
            .map_err(|e| {
                if e.is_connect() {
                    CliError::ConnectionRefused
                } else {
                    CliError::Api(e.to_string())
                }
            })?;

        return Ok(resp2);
    }

    Ok(resp)
}

/// Authenticated DELETE. Handles 401 with one retry.
#[allow(dead_code)]
pub fn authenticated_delete(url: &str) -> Result<reqwest::blocking::Response, CliError> {
    let token = get_auth_token()?;
    let client = http_client();

    let resp = client
        .delete(url)
        .header("X-Vault-Token", &token)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        clear_token();
        let new_password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
        let new_token = api_unlock(&new_password)?;
        save_token(&new_token);

        let resp2 = client
            .delete(url)
            .header("X-Vault-Token", &new_token)
            .send()
            .map_err(|e| {
                if e.is_connect() {
                    CliError::ConnectionRefused
                } else {
                    CliError::Api(e.to_string())
                }
            })?;

        return Ok(resp2);
    }

    Ok(resp)
}

/// GET /items — returns all items. Used for duplicate detection.
pub fn api_list_all_items() -> Result<Vec<ItemSummary>, CliError> {
    let url = format!("{API_BASE}/items");
    let resp = authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        return Err(CliError::Api(format!("HTTP {}", resp.status())));
    }

    resp.json().map_err(|e| CliError::Api(e.to_string()))
}

/// POST /items/:id/reveal — returns the secret value of an item.
pub fn api_reveal(item_id: i64, token: &str) -> Result<String, CliError> {
    let client = http_client();
    // confirm: true is required by the API to acknowledge the reveal action
    let body = serde_json::json!({ "confirm": true });
    let resp = client
        .post(format!("{API_BASE}/items/{}/reveal", item_id))
        .header("X-Vault-Token", token)
        .json(&body)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status().is_success() {
        let data: RevealResponse = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
        Ok(data.value)
    } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
        Err(CliError::NotFound("item".to_string()))
    } else {
        let err: ApiError = resp.json().unwrap_or(ApiError { error: "error".into() });
        Err(CliError::Api(err.error))
    }
}

/// Searches for an item by exact name (case-insensitive) and returns (id, secret value).
pub fn find_and_reveal(name: &str) -> Result<(i64, String), CliError> {
    let token = get_auth_token()?;
    let url = format!("{API_BASE}/items?search={}", urlencod(name));

    let client = http_client();
    let resp = client
        .get(&url)
        .header("X-Vault-Token", &token)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                CliError::ConnectionRefused
            } else {
                CliError::Api(e.to_string())
            }
        })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(CliError::Unauthorized);
    }
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("HTTP error {code}")));
    }

    let items: Vec<ItemSummary> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let name_lower = name.to_lowercase();

    let found = items.into_iter().find(|item| {
        item.name
            .as_deref()
            .map(|n| n.to_lowercase() == name_lower)
            .unwrap_or(false)
            || item
                .title
                .as_deref()
                .map(|t| t.to_lowercase() == name_lower)
                .unwrap_or(false)
    });

    let item = found.ok_or_else(|| CliError::NotFound(name.to_string()))?;
    let value = api_reveal(item.id, &token)?;
    Ok((item.id, value))
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Minimal URL encoding for item names in query strings.
pub fn urlencod(s: &str) -> String {
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

/// Parses --VAR=value style arguments into a HashMap.
pub fn parse_vars(vars: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for var in vars {
        let stripped = var.trim_start_matches('-');
        if let Some(eq_pos) = stripped.find('=') {
            let key = stripped[..eq_pos].to_string();
            let value = stripped[eq_pos + 1..].to_string();
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }
    map
}
