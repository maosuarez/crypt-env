//! Binario standalone CLI para la bóveda privada.
//! No depende de tauri_private_vault_lib — solo usa clap, reqwest::blocking,
//! rpassword y stdlib.

use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const API_BASE: &str = "http://127.0.0.1:47821";

// ─── Tipos de error ───────────────────────────────────────────────────────────

#[derive(Debug)]
enum CliError {
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
            CliError::Api(msg) => write!(f, "Error de API: {msg}"),
            CliError::Io(e) => write!(f, "Error de E/S: {e}"),
            CliError::ConnectionRefused => write!(
                f,
                "Error: la bóveda no está en ejecución. Abre la aplicación e inténtalo de nuevo."
            ),
            CliError::Unauthorized => write!(f, "Error: no autorizado (token inválido)"),
            CliError::NotFound(name) => write!(f, "Error: '{}' no encontrado en la bóveda", name),
            CliError::VaultLocked => write!(f, "Error: la bóveda está bloqueada"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

// ─── Tipos de respuesta de la API ─────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct UnlockResponse {
    token: String,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    error: String,
}

#[derive(Deserialize, Debug)]
struct ItemSummary {
    id: i64,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CommandDetail {
    id: i64,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    placeholders: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RevealResponse {
    value: String,
}

// ─── Token de sesión ──────────────────────────────────────────────────────────

fn token_path() -> Option<PathBuf> {
    // %APPDATA%\com.maosuarez.vault\.cli_token en Windows
    // ~/.local/share/com.maosuarez.vault/.cli_token como fallback
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("com.maosuarez.vault").join(".cli_token"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.maosuarez.vault")
                .join(".cli_token"),
        )
    } else {
        None
    }
}

fn read_token() -> Option<String> {
    let path = token_path()?;
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_token(token: &str) {
    if let Some(path) = token_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, token);
    }
}

fn clear_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(&path);
    }
}

// ─── Cliente HTTP ─────────────────────────────────────────────────────────────

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::new()
}

/// POST /unlock — retorna el token de sesión.
fn api_unlock(password: &str) -> Result<String, CliError> {
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
        let err: ApiError = resp.json().unwrap_or(ApiError { error: "error desconocido".into() });
        Err(CliError::Api(err.error))
    }
}

/// Obtiene un token válido: usa el guardado o pide la contraseña.
fn get_auth_token() -> Result<String, CliError> {
    if let Some(token) = read_token() {
        return Ok(token);
    }

    let password = rpassword::prompt_password("Master password: ").map_err(CliError::Io)?;
    let token = api_unlock(&password)?;
    save_token(&token);
    Ok(token)
}

/// Realiza una petición GET autenticada. Maneja 401 (limpia token y reintenta una vez).
fn authenticated_get(url: &str) -> Result<reqwest::blocking::Response, CliError> {
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
        // Token inválido o expirado — limpiar y reintentar con password
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

/// POST /items/:id/reveal — retorna el valor secreto del item.
fn api_reveal(item_id: i64, token: &str) -> Result<String, CliError> {
    let client = http_client();
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

/// Busca un item por nombre exacto (case-insensitive) y retorna su (id, valor secreto).
fn find_and_reveal(name: &str) -> Result<(i64, String), CliError> {
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
        return Err(CliError::Api(format!("error HTTP {code}")));
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

// ─── Estructura Clap ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "vault", about = "CLI para tu bóveda privada")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Rellena un archivo .env con secretos de la bóveda
    Fill {
        /// Ruta al archivo .env
        file: PathBuf,
    },
    /// Imprime export/env para evaluar en la terminal
    Set {
        /// Nombre del secreto
        name: String,
    },
    /// Gestiona comandos guardados en la bóveda
    Cmd {
        /// Nombre del comando (omitir para usar --list)
        name: Option<String>,
        /// Lista todos los comandos
        #[arg(long)]
        list: bool,
        /// Muestra detalles del comando (nombre, descripción, placeholders)
        #[arg(long)]
        info: bool,
        /// Variables para placeholders: --VAR=valor
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        vars: Vec<String>,
    },
    /// Busca ítems por nombre (sin exponer valores)
    Search {
        /// Texto a buscar
        query: String,
    },
}

// ─── Implementación de comandos ───────────────────────────────────────────────

fn cmd_fill(file: &Path) -> Result<(), CliError> {
    let content = std::fs::read_to_string(file)?;
    let lines: Vec<&str> = content.lines().collect();

    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut injected = 0usize;
    let mut not_found = 0usize;

    for line in &lines {
        let trimmed = line.trim();

        // Preservar comentarios y líneas vacías sin cambios
        if trimmed.is_empty() || trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            continue;
        }

        // Parsear KEY=value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = &trimmed[..eq_pos];

            // Validar que la clave es un identificador válido
            if key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                match find_and_reveal(key) {
                    Ok((_, value)) => {
                        new_lines.push(format!("{}={}", key, value));
                        injected += 1;
                        continue;
                    }
                    Err(CliError::NotFound(_)) => {
                        eprintln!("aviso: {} no encontrado en la bóveda", key);
                        not_found += 1;
                        new_lines.push(line.to_string());
                        continue;
                    }
                    Err(CliError::VaultLocked) => return Err(CliError::VaultLocked),
                    Err(e) => return Err(e),
                }
            }
        }

        new_lines.push(line.to_string());
    }

    let output = new_lines.join("\n");
    // Preservar newline final si el original lo tenía
    let output = if content.ends_with('\n') {
        format!("{}\n", output)
    } else {
        output
    };

    std::fs::write(file, output)?;
    eprintln!(
        "Ok .env actualizado ({} secretos inyectados, {} no encontrados)",
        injected, not_found
    );
    eprintln!(
        "Atención: '{}' contiene secretos en texto claro. Asegúrate de que sus permisos sean restrictivos y no lo compartas.",
        file.display()
    );
    Ok(())
}

fn cmd_set(name: &str) -> Result<(), CliError> {
    let (_, value) = find_and_reveal(name)?;

    // Detectar shell
    let is_powershell = std::env::var("PSModulePath").is_ok()
        || std::env::var("SHELL")
            .unwrap_or_default()
            .to_lowercase()
            .contains("powershell");

    if is_powershell {
        println!("$env:{} = '{}'", name, value.replace('\'', "''"));
    } else {
        // bash/zsh: usar comillas simples si hay espacios o caracteres especiales
        if value.contains(|c: char| c.is_whitespace() || "\"'\\$`!".contains(c)) {
            println!("export {}='{}'", name, value.replace('\'', "'\\''"));
        } else {
            println!("export {}={}", name, value);
        }
    }

    Ok(())
}

fn cmd_list_commands() -> Result<(), CliError> {
    let url = format!("{API_BASE}/commands");
    let resp = authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("error HTTP {code}")));
    }

    let commands: Vec<CommandDetail> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;

    if commands.is_empty() {
        eprintln!("No hay comandos guardados en la bóveda.");
        return Ok(());
    }

    println!("{:<24} {:<40} SHELL", "NOMBRE", "DESCRIPCIÓN");
    println!("{}", "-".repeat(80));
    for cmd in &commands {
        println!(
            "{:<24} {:<40} {}",
            cmd.name,
            cmd.description.as_deref().unwrap_or(""),
            cmd.shell.as_deref().unwrap_or("")
        );
    }

    Ok(())
}

fn cmd_command_info(name: &str) -> Result<(), CliError> {
    // Paso 1: GET /commands → encontrar por nombre
    let url = format!("{API_BASE}/commands");
    let resp = authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("error HTTP {code}")));
    }

    let commands: Vec<CommandDetail> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let name_lower = name.to_lowercase();

    let found = commands
        .into_iter()
        .find(|c| c.name.to_lowercase() == name_lower);

    let cmd_id = match found {
        Some(c) => c.id,
        None => return Err(CliError::NotFound(name.to_string())),
    };

    // Paso 2: GET /commands/:id → detalles con placeholders
    let url2 = format!("{API_BASE}/commands/{}", cmd_id);
    let resp2 = authenticated_get(&url2)?;

    if resp2.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CliError::NotFound(name.to_string()));
    }
    if !resp2.status().is_success() {
        let code = resp2.status();
        return Err(CliError::Api(format!("error HTTP {code}")));
    }

    let detail: CommandDetail = resp2.json().map_err(|e| CliError::Api(e.to_string()))?;
    let command_template = detail.command.as_deref().unwrap_or("");

    println!("Nombre:       {}", detail.name);
    println!("Descripción:  {}", detail.description.as_deref().unwrap_or(""));
    println!("Shell:        {}", detail.shell.as_deref().unwrap_or(""));
    println!("Comando:      {}", command_template);
    if !detail.placeholders.is_empty() {
        println!("Placeholders: {}", detail.placeholders.join(", "));
    }

    Ok(())
}

fn cmd_run_command(name: &str, vars: &[String]) -> Result<(), CliError> {
    // GET /commands → encontrar por nombre
    let url = format!("{API_BASE}/commands");
    let resp = authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("error HTTP {code}")));
    }

    let commands: Vec<CommandDetail> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;
    let name_lower = name.to_lowercase();

    let found = commands
        .into_iter()
        .find(|c| c.name.to_lowercase() == name_lower);

    let cmd = match found {
        Some(c) => c,
        None => return Err(CliError::NotFound(name.to_string())),
    };

    let mut template = cmd.command.unwrap_or_default();

    if !vars.is_empty() {
        let replacements = parse_vars(vars);
        for (key, val) in &replacements {
            let placeholder = format!("{{{{{}}}}}", key);
            template = template.replace(&placeholder, val);
        }
    }

    println!("{}", template);
    Ok(())
}

fn cmd_search(query: &str) -> Result<(), CliError> {
    let url = format!("{API_BASE}/items?search={}", urlencod(query));
    let resp = authenticated_get(&url)?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(CliError::VaultLocked);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        return Err(CliError::Api(format!("error HTTP {code}")));
    }

    let items: Vec<ItemSummary> = resp.json().map_err(|e| CliError::Api(e.to_string()))?;

    if items.is_empty() {
        eprintln!("No se encontraron resultados para '{}'.", query);
        return Ok(());
    }

    println!("{:<6} {:<16} {:<32} CATEGORÍAS", "ID", "TIPO", "NOMBRE/TÍTULO");
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

// ─── Utilidades ───────────────────────────────────────────────────────────────

/// Codificación URL mínima para nombres de items en query strings.
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

/// Parsea argumentos del tipo --VAR=valor a un HashMap.
fn parse_vars(vars: &[String]) -> HashMap<String, String> {
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

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Fill { file } => cmd_fill(&file),
        Commands::Set { name } => cmd_set(&name),
        Commands::Cmd { name: None, list: true, .. } => cmd_list_commands(),
        Commands::Cmd { name: Some(ref n), info: true, .. } => cmd_command_info(n),
        Commands::Cmd { name: Some(ref n), vars, .. } => cmd_run_command(n, &vars),
        Commands::Cmd { name: None, list: false, .. } => {
            eprintln!("Usa --list para listar comandos o proporciona un nombre.");
            std::process::exit(1);
        }
        Commands::Search { query } => cmd_search(&query),
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
