// crypt-env-mcp.rs — Standalone MCP server over stdio.
// Does not import from the project lib — uses only serde, serde_json, reqwest::blocking.

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

fn read_mcp_token() -> Result<String, String> {
    let path = std::env::var("APPDATA")
        .map(|d| std::path::PathBuf::from(d).join("com.maosuarez.cryptenv").join("mcp_token"))
        .or_else(|_| {
            std::env::var("HOME").map(|d| {
                std::path::PathBuf::from(d)
                    .join(".local")
                    .join("share")
                    .join("com.maosuarez.cryptenv")
                    .join("mcp_token")
            })
        })
        .map_err(|_| "cannot determine token path".to_string())?;

    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|_| {
            "MCP token not found. Generate one in vault Settings → INTEGRATIONS.".to_string()
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
            "name": "vault_list_items",
            "description": "Lista ítems de la bóveda. Nunca incluye campos secretos (value, password).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Filtrar por tipo: secret, credential, link, command, note" },
                    "category": { "type": "string", "description": "Filtrar por categoría" }
                }
            }
        },
        {
            "name": "vault_get_item",
            "description": "Retorna metadata de un ítem sin el valor secreto.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "ID del ítem" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "vault_generate_env",
            "description": "Genera un archivo .env con los valores reales de los secretos indicados. Retorna la ruta del archivo — los valores nunca aparecen en el resultado.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": { "type": "array", "items": { "type": "string" }, "description": "Nombres de los secretos a incluir" }
                },
                "required": ["keys"]
            }
        },
        {
            "name": "vault_inject_env",
            "description": "Inyecta un secreto como variable de entorno en el proceso MCP. No retorna el valor.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Nombre del secreto" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "vault_add_item",
            "description": "Agrega un nuevo ítem a la bóveda.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Tipo: secret, credential, link, command, note" },
                    "name": { "type": "string" },
                    "value": { "type": "string", "description": "Valor secreto (para secret/credential)" },
                    "category": { "type": "string" },
                    "notes": { "type": "string" },
                    "url": { "type": "string" },
                    "username": { "type": "string" }
                },
                "required": ["type", "name"]
            }
        },
        {
            "name": "vault_update_settings",
            "description": "Modifica configuración: auto_lock_timeout (minutos) y hotkey. No permite cambiar master_password.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "auto_lock_timeout": { "type": "integer", "description": "Minutos hasta auto-lock (0 = nunca)" },
                    "hotkey": { "type": "string", "description": "Atajo global, ej: Ctrl+Alt+Z" }
                }
            }
        },
        {
            "name": "vault_list_commands",
            "description": "Lista commands disponibles con nombre, descripción y placeholders requeridos.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vault_run_command",
            "description": "Retorna un Command resuelto sustituyendo los placeholders {{VAR}}. No lo ejecuta.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nombre del command" },
                    "params": {
                        "type": "object",
                        "description": "Mapa de VAR → valor para resolver placeholders",
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
                "vault no está en ejecución".to_string()
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
                "vault no está en ejecución".to_string()
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
                "vault no está en ejecución".to_string()
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
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo respuesta: {e}")),
    };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(
            serde_json::to_string_pretty(&v).unwrap_or(text),
        ),
        Err(_) => tool_ok(text),
    }
}

fn tool_get_item(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let id = match args.get("id").and_then(|v| v.as_i64()) {
        Some(i) => i,
        None => return tool_err("parámetro 'id' requerido"),
    };

    let resp = match vault_get(&format!("/items/{id}"), token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let status = resp.status().as_u16();
    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo respuesta: {e}")),
    };

    if status == 404 {
        return tool_err("ítem no encontrado");
    }
    if status == 403 {
        return tool_err("vault_locked: desbloquea la bóveda primero");
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
        None => return tool_err("parámetro 'keys' requerido (array de strings)"),
    };

    let mut env_lines: Vec<String> = Vec::new();
    let mut n_found = 0usize;

    for key in &keys {
        if !is_safe_env_key(key) {
            env_lines.push(format!("# {key}: nombre inválido o bloqueado, omitido"));
            continue;
        }

        // Buscar ítem por nombre exacto (case-insensitive via search)
        let search_url = format!("/items?search={}", urlencod(key));
        let items_resp = match vault_get(&search_url, token) {
            Ok(r) => r,
            Err(e) => return tool_err(e),
        };

        if items_resp.status().as_u16() == 403 {
            return tool_err("vault_locked: desbloquea la bóveda primero");
        }

        let items_text = match items_resp.text() {
            Ok(t) => t,
            Err(e) => return tool_err(format!("error leyendo items: {e}")),
        };

        let items_val: serde_json::Value = match serde_json::from_str(&items_text) {
            Ok(v) => v,
            Err(_) => {
                env_lines.push(format!("# {key}: error parseando respuesta"));
                continue;
            }
        };

        // Buscar ítem con nombre exacto (case-insensitive)
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
                env_lines.push(format!("# {key}: no encontrado en bóveda"));
                continue;
            }
        };

        // Revelar valor
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
            Err(e) => return tool_err(format!("error leyendo reveal: {e}")),
        };

        let reveal_val: serde_json::Value = match serde_json::from_str(&reveal_text) {
            Ok(v) => v,
            Err(_) => {
                env_lines.push(format!("# {key}: error parseando valor"));
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
    let filename = format!("vault_{}.env", random_hex(8));
    let path = std::env::temp_dir().join(&filename);

    if let Err(e) = std::fs::write(&path, &content) {
        return tool_err(format!("error escribiendo archivo .env: {e}"));
    }

    // Registrar para limpieza en la próxima llamada
    track_temp_file(path.clone());

    let path_str = path.to_string_lossy().to_string();
    tool_ok(
        serde_json::to_string_pretty(&serde_json::json!({
            "path": path_str,
            "count": n_found,
            "note": "Este archivo contiene secretos en texto claro. Cárgalo y elimínalo inmediatamente."
        }))
        .unwrap_or_default(),
    )
}

fn tool_inject_env(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return tool_err("parámetro 'key' requerido"),
    };

    if !is_safe_env_key(&key) {
        return tool_err(format!(
            "nombre de variable inválido o bloqueado: '{key}'. \
             Debe ser [A-Z][A-Z0-9_]* y no puede ser una variable de sistema crítica."
        ));
    }

    // Buscar ítem por nombre
    let search_url = format!("/items?search={}", urlencod(&key));
    let items_resp = match vault_get(&search_url, token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if items_resp.status().as_u16() == 403 {
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }

    let items_text = match items_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo items: {e}")),
    };

    let items_val: serde_json::Value = match serde_json::from_str(&items_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parseando lista de items"),
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
        None => return tool_err(format!("secreto '{key}' no encontrado en bóveda")),
    };

    // Revelar valor
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
        Err(e) => return tool_err(format!("error leyendo reveal: {e}")),
    };

    let reveal_val: serde_json::Value = match serde_json::from_str(&reveal_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parseando valor revelado"),
    };

    let value = match reveal_val.get("value").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return tool_err("el ítem no tiene valor secreto"),
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
        None => return tool_err("parámetro 'type' requerido"),
    };
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return tool_err("parámetro 'name' requerido"),
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
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }
    if status >= 400 {
        return tool_err(format!("error creando ítem (HTTP {status}): {text}"));
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
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }
    if status >= 400 {
        let text = resp.text().unwrap_or_default();
        return tool_err(format!("error actualizando settings (HTTP {status}): {text}"));
    }

    tool_ok(
        serde_json::to_string_pretty(&serde_json::json!({ "ok": true })).unwrap_or_default(),
    )
}

fn tool_list_commands(token: &str) -> serde_json::Value {
    let resp = match vault_get("/commands", token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if resp.status().as_u16() == 403 {
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }

    let text = match resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo respuesta: {e}")),
    };

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => tool_ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => tool_ok(text),
    }
}

fn tool_run_command(args: &serde_json::Value, token: &str) -> serde_json::Value {
    let cmd_name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return tool_err("parámetro 'name' requerido"),
    };

    // Obtener lista de commands
    let list_resp = match vault_get("/commands", token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    if list_resp.status().as_u16() == 403 {
        return tool_err("vault_locked: desbloquea la bóveda primero");
    }

    let list_text = match list_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo commands: {e}")),
    };

    let list_val: serde_json::Value = match serde_json::from_str(&list_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parseando lista de commands"),
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
        None => return tool_err(format!("comando '{cmd_name}' no encontrado")),
    };

    // Obtener detalle del command
    let detail_resp = match vault_get(&format!("/commands/{cmd_id}"), token) {
        Ok(r) => r,
        Err(e) => return tool_err(e),
    };

    let detail_text = match detail_resp.text() {
        Ok(t) => t,
        Err(e) => return tool_err(format!("error leyendo detalle: {e}")),
    };

    let detail_val: serde_json::Value = match serde_json::from_str(&detail_text) {
        Ok(v) => v,
        Err(_) => return tool_err("error parseando detalle del command"),
    };

    let template = match detail_val.get("command").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return tool_err("el command no tiene template"),
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
            "serverInfo": { "name": "crypt-env-mcp", "version": "0.1.0" }
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
        "vault_list_items" => tool_list_items(args, token),
        "vault_get_item" => tool_get_item(args, token),
        "vault_generate_env" => tool_generate_env(args, token),
        "vault_inject_env" => tool_inject_env(args, token),
        "vault_add_item" => tool_add_item(args, token),
        "vault_update_settings" => tool_update_settings(args, token),
        "vault_list_commands" => tool_list_commands(token),
        "vault_run_command" => tool_run_command(args, token),
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
