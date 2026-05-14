use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router, middleware};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::crypto;
use crate::db::DbCategory;
use crate::share::{ShareState, ShareSessionState};
use crate::tls;
use crate::vault::{SharedState, VaultItem};

// ─── Estado compartido de la API ──────────────────────────────────────────────

struct RateLimitState {
    attempts: u32,
    window_start: Instant,
}

pub struct ApiState {
    vault: SharedState,
    session_token: Arc<Mutex<Option<String>>>,
    token_expires: Arc<Mutex<Option<Instant>>>,
    unlock_rate: Mutex<RateLimitState>,
    share: Arc<ShareState>,
}

// ─── Tipos de respuesta ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    code: String,
}

#[derive(Serialize)]
struct UnlockResponse {
    token: String,
}

#[derive(Serialize)]
struct CategoryResponse {
    id: String,
    name: String,
    color: String,
}

#[derive(Serialize)]
struct CommandDetail {
    id: i64,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    placeholders: Vec<String>,
}

#[derive(Serialize)]
struct RevealResponse {
    value: String,
}

// ─── CORS guard ───────────────────────────────────────────────────────────────

/// Rechaza requests con Origin distinto de "null" o ausente (browser cross-origin).
/// Añade Access-Control-Allow-Origin: null a todas las respuestas.
async fn cors_guard(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let allowed = match request.headers().get("origin") {
        Some(origin) => origin.as_bytes() == b"null",
        None => true,
    };
    if !allowed {
        return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
    }
    let mut response = next.run(request).await;
    if let Ok(val) = HeaderValue::from_str("null") {
        response.headers_mut().insert("access-control-allow-origin", val);
    }
    response
}

// ─── Helpers de respuesta y auth ──────────────────────────────────────────────

fn err_json(status: StatusCode, msg: &str, code: &str) -> impl IntoResponse {
    (status, Json(ErrorBody { error: msg.to_string(), code: code.to_string() }))
}

/// Verifica el header X-Vault-Token.
/// Acepta session token (con expiración) o MCP token estático (sin expiración).
async fn verify_token(headers: &HeaderMap, state: &ApiState) -> Result<(), StatusCode> {
    let provided = headers
        .get("x-vault-token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // --- Verificar session token (con expiración) ---
    {
        let stored = state.session_token.lock().await;
        let expires = state.token_expires.lock().await;

        if let (Some(s), Some(exp)) = (stored.as_deref(), *expires) {
            if Instant::now() < exp && bool::from(s.as_bytes().ct_eq(provided.as_bytes())) {
                let vault = state.vault.lock().await;
                return if vault.key.is_some() { Ok(()) } else { Err(StatusCode::FORBIDDEN) };
            }
        }
    }

    // --- Fallback: MCP token estático (sin expiración) ---
    {
        let vault = state.vault.lock().await;
        if vault.key.is_none() {
            return Err(StatusCode::FORBIDDEN);
        }
        let mcp_token = vault.db.get_setting("mcp_token").await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        match mcp_token {
            Some(t) if bool::from(t.as_bytes().ct_eq(provided.as_bytes())) => Ok(()),
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

/// Toma el lock del vault, copia key + raw data, suelta el lock, descifra sin lock.
async fn decrypt_all_items(state: &ApiState) -> Result<Vec<VaultItem>, StatusCode> {
    // Fase 1: tomar lock, extraer key y datos crudos
    let (key, raw) = {
        let vault = state.vault.lock().await;
        let key = vault.key.as_ref().ok_or(StatusCode::FORBIDDEN)?.clone();
        let raw = vault
            .db
            .list_items()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        (key, raw)
        // lock se suelta aquí
    };

    // Fase 2: descifrar sin lock
    let items: Vec<VaultItem> = raw
        .into_iter()
        .filter_map(|(id, _, data, _)| {
            let json = crypto::decrypt(&key, &data).ok()?;
            let mut item: VaultItem = serde_json::from_slice(&json).ok()?;
            item.id = id;
            Some(item)
        })
        .collect();

    Ok(items)
}

/// Elimina los campos sensibles antes de retornar un item al cliente.
fn redact_item(mut item: VaultItem) -> VaultItem {
    item.value = None;
    item.password = None;
    item.content = None;
    item
}

/// Extrae placeholders del formato {{VAR}} de un template.
fn extract_placeholders(template: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut s = template;
    while let Some(start) = s.find("{{") {
        let rest = &s[start + 2..];
        if let Some(end) = rest.find("}}") {
            let name = &rest[..end];
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let ph = format!("{{{{{}}}}}", name);
                if !result.contains(&ph) {
                    result.push(ph);
                }
            }
            s = &rest[end + 2..];
        } else {
            break;
        }
    }
    result
}

fn now_ts_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ─── Input validation ─────────────────────────────────────────────────────────

const VALID_ITEM_TYPES: &[&str] = &["secret", "credential", "link", "note", "command"];

/// Returns an HTTP 422 response with the standard error body.
fn err_validation(field: &str, reason: &str) -> axum::response::Response {
    err_json(
        StatusCode::UNPROCESSABLE_ENTITY,
        &format!("{field}: {reason}"),
        "VALIDATION_ERROR",
    )
    .into_response()
}

/// Validates fields for a POST /items (create) request.
/// All required fields must be present and non-empty; optional fields are
/// validated only when present.
fn validate_create(body: &VaultItem) -> Result<(), axum::response::Response> {
    // name: required, non-empty, max 255
    match body.name.as_deref() {
        None | Some("") => return Err(err_validation("name", "required and must not be empty")),
        Some(n) if n.len() > 255 => return Err(err_validation("name", "must be 255 characters or fewer")),
        _ => {}
    }

    // type: required, must be a known variant
    if body.item_type.is_empty() {
        return Err(err_validation("type", "required"));
    }
    if !VALID_ITEM_TYPES.contains(&body.item_type.as_str()) {
        return Err(err_validation(
            "type",
            &format!(
                "must be one of: {}",
                VALID_ITEM_TYPES.join(", ")
            ),
        ));
    }

    // value: required, non-empty
    match body.value.as_deref() {
        None | Some("") => return Err(err_validation("value", "required and must not be empty")),
        _ => {}
    }

    // categories: each entry max 100 chars
    for cat in &body.categories {
        if cat.len() > 100 {
            return Err(err_validation("category", "each entry must be 100 characters or fewer"));
        }
    }

    Ok(())
}

/// Validates fields for a PUT /items/:id (update) request.
/// All fields are optional, but any field that is present must satisfy its rule.
fn validate_update(body: &VaultItem) -> Result<(), axum::response::Response> {
    // name: if present must be non-empty and max 255
    if let Some(n) = body.name.as_deref() {
        if n.is_empty() {
            return Err(err_validation("name", "must not be empty"));
        }
        if n.len() > 255 {
            return Err(err_validation("name", "must be 255 characters or fewer"));
        }
    }

    // type: if present (non-empty string sent) must be a known variant
    if !body.item_type.is_empty() && !VALID_ITEM_TYPES.contains(&body.item_type.as_str()) {
        return Err(err_validation(
            "type",
            &format!(
                "must be one of: {}",
                VALID_ITEM_TYPES.join(", ")
            ),
        ));
    }

    // value: if present must be non-empty
    if let Some(v) = body.value.as_deref() {
        if v.is_empty() {
            return Err(err_validation("value", "must not be empty"));
        }
    }

    // categories: each entry max 100 chars
    for cat in &body.categories {
        if cat.len() > 100 {
            return Err(err_validation("category", "each entry must be 100 characters or fewer"));
        }
    }

    Ok(())
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UnlockBody {
    master_password: String,
}

async fn handle_unlock(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<UnlockBody>,
) -> impl IntoResponse {
    // Rate limiting: máx 5 intentos de unlock por ventana de 60 segundos
    {
        let mut rate = state.unlock_rate.lock().await;
        let now = Instant::now();
        if now.duration_since(rate.window_start) >= Duration::from_secs(60) {
            rate.window_start = now;
            rate.attempts = 0;
        }
        if rate.attempts >= 5 {
            let elapsed = now.duration_since(rate.window_start).as_secs();
            let retry_after = 60u64.saturating_sub(elapsed);
            let mut resp = err_json(
                StatusCode::TOO_MANY_REQUESTS,
                "demasiados intentos, reintenta más tarde",
                "RATE_LIMITED",
            )
            .into_response();
            if let Ok(val) = HeaderValue::from_str(&retry_after.to_string()) {
                resp.headers_mut().insert("retry-after", val);
            }
            return resp;
        }
        rate.attempts += 1;
    }

    let mut vault = state.vault.lock().await;

    // Verificar que la bóveda está inicializada
    let (salt, token) = match vault.db.get_meta().await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return err_json(
                StatusCode::BAD_REQUEST,
                "bóveda no inicializada",
                "BAD_REQUEST",
            )
            .into_response()
        }
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR")
                .into_response()
        }
    };

    let key = match crypto::unlock_vault_crypto(body.master_password.as_bytes(), &salt, &token) {
        Ok(k) => k,
        Err(_) => {
            return err_json(
                StatusCode::BAD_REQUEST,
                "contraseña incorrecta",
                "BAD_REQUEST",
            )
            .into_response()
        }
    };

    vault.key = Some(Zeroizing::new(key));

    // Leer timeout de la DB (minutos, default 5)
    let minutes = match vault.db.get_setting("auto_lock_timeout").await {
        Ok(Some(v)) => v.parse::<u64>().unwrap_or(5),
        _ => 5,
    };

    drop(vault);

    // Generar token de sesión: 16 bytes = 32 chars hex
    let mut token_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let session_token: String = token_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    let expires = Instant::now() + Duration::from_secs(minutes * 60);

    *state.session_token.lock().await = Some(session_token.clone());
    *state.token_expires.lock().await = Some(expires);

    (StatusCode::OK, Json(UnlockResponse { token: session_token })).into_response()
}

#[derive(Deserialize)]
struct ItemsQuery {
    #[serde(rename = "type")]
    item_type: Option<String>,
    category: Option<String>,
    search: Option<String>,
}

async fn handle_list_items(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(params): Query<ItemsQuery>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    let type_filter = params.item_type.as_deref().map(|s| s.to_lowercase());
    let cat_filter = params.category.as_deref().map(|s| s.to_lowercase());
    let search_filter = params.search.as_deref().map(|s| s.to_lowercase());

    let filtered: Vec<VaultItem> = items
        .into_iter()
        .filter(|item| {
            // Filtro por tipo
            if let Some(ref t) = type_filter {
                if item.item_type.to_lowercase() != *t {
                    return false;
                }
            }
            // Filtro por categoría
            if let Some(ref cat) = cat_filter {
                let found = item
                    .categories
                    .iter()
                    .any(|c| c.to_lowercase() == *cat);
                if !found {
                    return false;
                }
            }
            // Filtro por búsqueda en nombre/título
            if let Some(ref q) = search_filter {
                let name_match = item
                    .name
                    .as_deref()
                    .map(|n| n.to_lowercase().contains(q.as_str()))
                    .unwrap_or(false);
                let title_match = item
                    .title
                    .as_deref()
                    .map(|t| t.to_lowercase().contains(q.as_str()))
                    .unwrap_or(false);
                if !name_match && !title_match {
                    return false;
                }
            }
            true
        })
        .map(redact_item)
        .collect();

    (StatusCode::OK, Json(filtered)).into_response()
}

async fn handle_get_item(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    match items.into_iter().find(|item| item.id == id) {
        Some(item) => (StatusCode::OK, Json(redact_item(item))).into_response(),
        None => err_json(StatusCode::NOT_FOUND, "item no encontrado", "NOT_FOUND").into_response(),
    }
}

async fn handle_create_item(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(mut body): Json<VaultItem>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if let Err(resp) = validate_create(&body) {
        return resp;
    }

    // Asegurar timestamp de creación
    if body.created.is_empty() {
        body.created = now_ts_str();
    }

    let (key, new_id) = {
        let vault = state.vault.lock().await;
        let key = match vault.key.as_ref() {
            Some(k) => k.clone(),
            None => {
                return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                    .into_response()
            }
        };

        let json = match serde_json::to_vec(&body) {
            Ok(j) => j,
            Err(_) => {
                return err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "error serializando item",
                    "INTERNAL_ERROR",
                )
                .into_response()
            }
        };

        let encrypted = match crypto::encrypt(&key, &json) {
            Ok(e) => e,
            Err(_) => {
                return err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "error cifrando item",
                    "INTERNAL_ERROR",
                )
                .into_response()
            }
        };

        let new_id = match vault
            .db
            .upsert_item(0, &body.item_type, &encrypted, &body.created)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR")
                    .into_response()
            }
        };

        (key, new_id)
    };

    let _ = key; // ya no necesitamos la key
    body.id = new_id;
    (StatusCode::CREATED, Json(redact_item(body))).into_response()
}

async fn handle_update_item(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(mut body): Json<VaultItem>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if let Err(resp) = validate_update(&body) {
        return resp;
    }

    // Verificar que el item existe
    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    if items.iter().find(|item| item.id == id).is_none() {
        return err_json(StatusCode::NOT_FOUND, "item no encontrado", "NOT_FOUND").into_response();
    }

    body.id = id;

    let vault = state.vault.lock().await;
    let key = match vault.key.as_ref() {
        Some(k) => k.clone(),
        None => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
    };

    let json = match serde_json::to_vec(&body) {
        Ok(j) => j,
        Err(_) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "error serializando item",
                "INTERNAL_ERROR",
            )
            .into_response()
        }
    };

    let encrypted = match crypto::encrypt(&key, &json) {
        Ok(e) => e,
        Err(_) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "error cifrando item",
                "INTERNAL_ERROR",
            )
            .into_response()
        }
    };

    match vault
        .db
        .upsert_item(id, &body.item_type, &encrypted, &body.created)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR")
                .into_response()
        }
    }

    (StatusCode::OK, Json(redact_item(body))).into_response()
}

async fn handle_delete_item(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    // Verificar que el item existe
    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    if items.iter().find(|item| item.id == id).is_none() {
        return err_json(StatusCode::NOT_FOUND, "item no encontrado", "NOT_FOUND").into_response();
    }

    let vault = state.vault.lock().await;
    match vault.db.delete_item(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response()
        }
    }
}

async fn handle_list_categories(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Categorías no requieren key, solo token válido
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => {
                // Para categorías, FORBIDDEN (vault sin key) también es aceptable
                // pero el verify_token ya verifica key, así que devolvemos el error
                ("bóveda bloqueada", "VAULT_LOCKED")
            }
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let vault = state.vault.lock().await;
    match vault.db.list_categories().await {
        Ok(cats) => {
            let response: Vec<CategoryResponse> = cats
                .into_iter()
                .map(|c| CategoryResponse { id: c.cid, name: c.name, color: c.color })
                .collect();
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateCategoryBody {
    name: String,
    color: String,
}

async fn handle_create_category(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<CreateCategoryBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if body.name.is_empty() {
        return err_validation("name", "required and must not be empty");
    }
    if body.name.len() > 100 {
        return err_validation("name", "must be 100 characters or fewer");
    }
    if body.color.is_empty() {
        return err_validation("color", "required and must not be empty");
    }

    // Generate a UUID-like cid using random bytes
    let mut id_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    let cid = id_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();

    let cat = DbCategory { cid: cid.clone(), name: body.name.clone(), color: body.color.clone() };

    let vault = state.vault.lock().await;
    match vault.db.insert_category(&cat).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(CategoryResponse { id: cid, name: body.name, color: body.color }),
        )
            .into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateCategoryBody {
    name: Option<String>,
    color: Option<String>,
}

async fn handle_update_category(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateCategoryBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    // Fetch existing category to merge fields
    let vault = state.vault.lock().await;
    let cats = match vault.db.list_categories().await {
        Ok(c) => c,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR")
                .into_response()
        }
    };

    let existing = match cats.into_iter().find(|c| c.cid == id) {
        Some(c) => c,
        None => {
            return err_json(StatusCode::NOT_FOUND, "category not found", "NOT_FOUND")
                .into_response()
        }
    };

    let new_name = body.name.unwrap_or(existing.name);
    let new_color = body.color.unwrap_or(existing.color);

    if new_name.is_empty() {
        return err_validation("name", "must not be empty");
    }
    if new_name.len() > 100 {
        return err_validation("name", "must be 100 characters or fewer");
    }

    let updated = DbCategory { cid: id.clone(), name: new_name.clone(), color: new_color.clone() };
    match vault.db.update_category(&updated).await {
        Ok(true) => (
            StatusCode::OK,
            Json(CategoryResponse { id, name: new_name, color: new_color }),
        )
            .into_response(),
        Ok(false) => {
            err_json(StatusCode::NOT_FOUND, "category not found", "NOT_FOUND").into_response()
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response(),
    }
}

async fn handle_delete_category(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let vault = state.vault.lock().await;
    match vault.db.delete_category(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => {
            err_json(StatusCode::NOT_FOUND, "category not found", "NOT_FOUND").into_response()
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response(),
    }
}

async fn handle_list_commands(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    let commands: Vec<CommandDetail> = items
        .into_iter()
        .filter(|item| item.item_type == "command")
        .map(|item| {
            let template = item.command.as_deref().unwrap_or("");
            let placeholders = extract_placeholders(template);
            CommandDetail {
                id: item.id,
                name: item.name.unwrap_or_default(),
                description: item.description,
                shell: item.shell,
                command: item.command,
                placeholders,
            }
        })
        .collect();

    (StatusCode::OK, Json(commands)).into_response()
}

async fn handle_get_command(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    let found = items
        .into_iter()
        .find(|item| item.item_type == "command" && item.id == id);

    match found {
        Some(item) => {
            let template = item.command.as_deref().unwrap_or("");
            let placeholders = extract_placeholders(template);
            let detail = CommandDetail {
                id: item.id,
                name: item.name.unwrap_or_default(),
                description: item.description,
                shell: item.shell,
                command: item.command,
                placeholders,
            };
            (StatusCode::OK, Json(detail)).into_response()
        }
        None => {
            err_json(StatusCode::NOT_FOUND, "comando no encontrado", "NOT_FOUND").into_response()
        }
    }
}

#[derive(Deserialize)]
struct RevealBody {
    confirm: Option<bool>,
}

async fn handle_reveal_item(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<RevealBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("no autorizado", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("bóveda bloqueada", "VAULT_LOCKED"),
            _ => ("error interno", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if body.confirm != Some(true) {
        return err_json(
            StatusCode::BAD_REQUEST,
            "se requiere confirm: true",
            "BAD_REQUEST",
        )
        .into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "bóveda bloqueada", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "error interno", "INTERNAL_ERROR")
                .into_response()
        }
    };

    match items.into_iter().find(|item| item.id == id) {
        Some(item) => {
            eprintln!("[reveal] item#{} at {}", id, now_ts_str());

            let value = item
                .value
                .or(item.password)
                .or(item.content)
                .unwrap_or_default();

            (StatusCode::OK, Json(RevealResponse { value })).into_response()
        }
        None => err_json(StatusCode::NOT_FOUND, "item no encontrado", "NOT_FOUND").into_response(),
    }
}

async fn handle_get_settings(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        return err_json(code, "no autorizado", "UNAUTHORIZED").into_response();
    }
    let vault = state.vault.lock().await;
    let timeout = vault.db.get_setting("auto_lock_timeout").await
        .unwrap_or_default()
        .unwrap_or_else(|| "5".into());
    let hotkey = vault.db.get_setting("hotkey").await
        .unwrap_or_default()
        .unwrap_or_else(|| "Ctrl+Alt+Z".into());
    drop(vault);

    (StatusCode::OK, Json(serde_json::json!({
        "auto_lock_timeout": timeout.parse::<i64>().unwrap_or(5),
        "hotkey": hotkey,
    }))).into_response()
}

#[derive(Deserialize)]
struct UpdateSettingsBody {
    auto_lock_timeout: Option<i64>,
    hotkey: Option<String>,
}

async fn handle_put_settings(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<UpdateSettingsBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        return err_json(code, "no autorizado", "UNAUTHORIZED").into_response();
    }
    let vault = state.vault.lock().await;
    if let Some(t) = body.auto_lock_timeout {
        if let Err(e) = vault.db.set_setting("auto_lock_timeout", &t.to_string()).await {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response();
        }
    }
    if let Some(h) = body.hotkey {
        if let Err(e) = vault.db.set_setting("hotkey", &h).await {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e, "INTERNAL_ERROR").into_response();
        }
    }
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// ─── /health ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    version: &'static str,
    status: &'static str,
    vault_locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_count: Option<usize>,
    mcp_token_configured: bool,
}

async fn handle_health(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    let vault = state.vault.lock().await;
    let vault_locked = vault.key.is_none();
    let item_count: Option<usize> = if !vault_locked {
        vault.db.list_items().await.ok().map(|items| items.len())
    } else {
        None
    };
    let mcp_token_configured = vault
        .db
        .get_setting("mcp_token")
        .await
        .ok()
        .flatten()
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    drop(vault);

    (
        StatusCode::OK,
        Json(HealthResponse {
            version: env!("CARGO_PKG_VERSION"),
            status: "running",
            vault_locked,
            item_count,
            mcp_token_configured,
        }),
    )
        .into_response()
}

// ─── TempEnvFile — RAII guard for secret-bearing files ───────────────────────
//
// Ensures that a file containing plaintext secrets is always zeroed and deleted
// when it goes out of scope — even on panic or early error return.
//
// Usage pattern:
//   let guard = TempEnvFile::create(path, content)?;
//   // ... any fallible work ...
//   let path = guard.persist(); // disarms the guard; caller now owns the file
//
// If `persist()` is never called (error path or panic), `Drop` wipes the file.

struct TempEnvFile {
    path: std::path::PathBuf,
    /// Byte length of the content written, used for the zero-overwrite pass.
    content_len: usize,
    /// Set to true by `persist()` to suppress cleanup in `Drop`.
    persisted: bool,
}

impl TempEnvFile {
    /// Write `content` to `path` and return a guard that will clean up on drop.
    ///
    /// On Unix the file is created with mode 0o600 (owner read/write only),
    /// preventing other users on the same system from reading the secrets.
    ///
    /// On Windows, `%APPDATA%` and other per-user directories already inherit
    /// restrictive NTFS ACLs from their parent — only the owning user account
    /// and SYSTEM have access. The standard library provides no portable API
    /// for setting Windows ACLs from Rust, so no additional permission change
    /// is needed or applied here.
    fn create(path: std::path::PathBuf, content: &str) -> Result<Self, std::io::Error> {
        std::fs::write(&path, content.as_bytes())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(Self {
            path,
            content_len: content.len(),
            persisted: false,
        })
    }

    /// Disarm the guard: the file will NOT be deleted on drop.
    /// Returns the path so the caller can report it.
    fn persist(mut self) -> std::path::PathBuf {
        self.persisted = true;
        // We consume `self` so Drop still runs, but the persisted flag prevents
        // any cleanup. Clone the path before consumption.
        self.path.clone()
    }
}

impl Drop for TempEnvFile {
    fn drop(&mut self) {
        if self.persisted {
            return;
        }
        // Overwrite with zeros first to hinder file-system recovery of secrets.
        // Use max(content_len, 1) so we always issue at least one write attempt
        // even if content_len is somehow zero.
        let zeros = vec![0u8; self.content_len.max(1)];
        let _ = std::fs::write(&self.path, &zeros);
        let _ = std::fs::remove_file(&self.path);
    }
}

// ─── /fill ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FillBody {
    template: String,
    /// When provided, write the filled .env directly to this path.
    /// The response will contain stats but not the secret content.
    output_path: Option<String>,
}

#[derive(Serialize)]
struct FillResponse {
    /// Only present when output_path is not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// Only present when output_path is specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    injected: usize,
    not_found: usize,
    missing_keys: Vec<String>,
}

async fn handle_fill(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<FillBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let items = match decrypt_all_items(&state).await {
        Ok(i) => i,
        Err(StatusCode::FORBIDDEN) => {
            return err_json(StatusCode::FORBIDDEN, "vault locked", "VAULT_LOCKED")
                .into_response()
        }
        Err(_) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error",
                "INTERNAL_ERROR",
            )
            .into_response()
        }
    };

    let mut new_lines: Vec<String> = Vec::new();
    let mut injected = 0usize;
    let mut not_found = 0usize;
    let mut missing_keys: Vec<String> = Vec::new();

    for line in body.template.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = &trimmed[..eq_pos];
            if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let key_lower = key.to_lowercase();
                let found = items.iter().find(|item| {
                    item.name
                        .as_deref()
                        .map(|n| n.to_lowercase() == key_lower)
                        .unwrap_or(false)
                });
                if let Some(item) = found {
                    let value = item
                        .value
                        .as_deref()
                        .or(item.password.as_deref())
                        .or(item.content.as_deref())
                        .unwrap_or("");
                    new_lines.push(format!("{key}={value}"));
                    injected += 1;
                    continue;
                } else {
                    missing_keys.push(key.to_string());
                    not_found += 1;
                    new_lines.push(format!("{key}="));
                    continue;
                }
            }
        }
        new_lines.push(line.to_string());
    }

    let mut filled = new_lines.join("\n");
    if body.template.ends_with('\n') {
        filled.push('\n');
    }

    // When output_path is given: write to disk via RAII guard, return stats only
    // (no secret content in the response).
    //
    // The guard zeros and deletes the file if any error occurs before persist().
    // On success, persist() disarms the guard so the caller can consume the file.
    if let Some(ref out) = body.output_path {
        let path = std::path::PathBuf::from(out);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("cannot create directory: {e}"),
                    "INTERNAL_ERROR",
                )
                .into_response();
            }
        }
        let guard = match TempEnvFile::create(path, &filled) {
            Ok(g) => g,
            Err(e) => {
                return err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("cannot write file: {e}"),
                    "INTERNAL_ERROR",
                )
                .into_response();
            }
        };
        // Disarm: caller is now responsible for the file.
        let final_path = guard.persist();
        return (
            StatusCode::OK,
            Json(FillResponse {
                content: None,
                path: Some(final_path.to_string_lossy().into_owned()),
                injected,
                not_found,
                missing_keys,
            }),
        )
            .into_response();
    }

    // Without output_path: return the content inline (CLI / programmatic use).
    (
        StatusCode::OK,
        Json(FillResponse {
            content: Some(filled),
            path: None,
            injected,
            not_found,
            missing_keys,
        }),
    )
        .into_response()
}

// ─── Share handlers ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ShareListenBody {
    items: Vec<i64>,
}

#[derive(Serialize)]
struct ShareListenResponse {
    pairing_code: String,
    fingerprint: Option<String>,
    expires_in: u64,
    status: &'static str,
}

async fn handle_share_listen(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<ShareListenBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if body.items.is_empty() {
        return err_json(StatusCode::UNPROCESSABLE_ENTITY, "items list must not be empty", "VALIDATION_ERROR").into_response();
    }

    // Extract vault key
    let vault_key: [u8; 32] = {
        let guard = state.vault.lock().await;
        match guard.key.as_ref() {
            Some(k) => **k,
            None => return err_json(StatusCode::FORBIDDEN, "vault locked", "VAULT_LOCKED").into_response(),
        }
    };

    let pairing_code = match crate::share::start_listen_session(
        state.share.clone(),
        body.items,
        vault_key,
        state.vault.clone(),
    )
    .await
    {
        Ok(code) => code,
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
                "INTERNAL_ERROR",
            )
            .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(ShareListenResponse {
            pairing_code,
            fingerprint: None,
            expires_in: 300,
            status: "listening",
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
struct ShareConnectBody {
    pairing_code: String,
}

#[derive(Serialize)]
struct ShareConnectResponse {
    fingerprint: String,
    status: &'static str,
}

async fn handle_share_connect(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<ShareConnectBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let vault_key: [u8; 32] = {
        let guard = state.vault.lock().await;
        match guard.key.as_ref() {
            Some(k) => **k,
            None => return err_json(StatusCode::FORBIDDEN, "vault locked", "VAULT_LOCKED").into_response(),
        }
    };

    let fingerprint = match crate::share::connect_to_peer(
        state.share.clone(),
        body.pairing_code,
        vault_key,
        state.vault.clone(),
    )
    .await
    {
        Ok(fp) => fp,
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
                "INTERNAL_ERROR",
            )
            .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(ShareConnectResponse {
            fingerprint,
            status: "awaiting_confirmation",
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
struct ShareConfirmBody {
    confirmed: bool,
}

#[derive(Serialize)]
struct ShareConfirmResponse {
    status: String,
}

async fn handle_share_confirm(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<ShareConfirmBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    match crate::share::confirm_fingerprint(&state.share, body.confirmed).await {
        Ok(()) => {
            let status = if body.confirmed { "active" } else { "cancelled" };
            (StatusCode::OK, Json(ShareConfirmResponse { status: status.to_string() })).into_response()
        }
        Err(e) => err_json(StatusCode::BAD_REQUEST, &e.to_string(), "BAD_REQUEST").into_response(),
    }
}

#[derive(Serialize)]
struct ShareStatusResponse {
    state: String,
    fingerprint: Option<String>,
    direction: Option<String>,
    received_names: Option<Vec<String>>,
}

async fn handle_share_status(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let guard = state.share.session.lock().await;
    match guard.as_ref() {
        None => (
            StatusCode::OK,
            Json(ShareStatusResponse {
                state: "none".to_string(),
                fingerprint: None,
                direction: None,
                received_names: None,
            }),
        )
            .into_response(),
        Some(s) => {
            let state_str = match &s.state {
                ShareSessionState::Failed(msg) => format!("failed: {msg}"),
                other => other.as_str().to_string(),
            };
            let received = if s.state == ShareSessionState::Done && s.direction == crate::share::ShareDirection::Receiving {
                Some(s.received_names.clone())
            } else {
                None
            };
            (
                StatusCode::OK,
                Json(ShareStatusResponse {
                    state: state_str,
                    fingerprint: s.fingerprint.clone(),
                    direction: Some(s.direction.as_str().to_string()),
                    received_names: received,
                }),
            )
                .into_response()
        }
    }
}

#[derive(Serialize)]
struct ShareCancelResponse {
    cancelled: bool,
}

async fn handle_share_cancel(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    match crate::share::cancel_session(&state.share).await {
        Ok(()) => (StatusCode::OK, Json(ShareCancelResponse { cancelled: true })).into_response(),
        Err(e) => err_json(StatusCode::BAD_REQUEST, &e.to_string(), "BAD_REQUEST").into_response(),
    }
}

#[derive(Deserialize)]
struct ShareExportBody {
    items: Vec<i64>,
    output_path: String,
}

#[derive(Serialize)]
struct ShareExportResponse {
    passphrase: String,
    path: String,
}

async fn handle_share_export(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<ShareExportBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    if body.items.is_empty() {
        return err_json(StatusCode::UNPROCESSABLE_ENTITY, "items list must not be empty", "VALIDATION_ERROR").into_response();
    }

    let output_path = std::path::PathBuf::from(&body.output_path);
    if let Some(parent) = output_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("cannot create output directory: {e}"),
                "INTERNAL_ERROR",
            )
            .into_response();
        }
    }

    match crate::share::export_package(&body.items, &output_path, &state.vault).await {
        Ok(passphrase) => (
            StatusCode::OK,
            Json(ShareExportResponse {
                passphrase,
                path: body.output_path,
            }),
        )
            .into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), "INTERNAL_ERROR")
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ShareImportBody {
    path: String,
    passphrase: String,
}

#[derive(Serialize)]
struct ShareImportResponse {
    imported: usize,
    item_names: Vec<String>,
}

async fn handle_share_import(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<ShareImportBody>,
) -> impl IntoResponse {
    if let Err(code) = verify_token(&headers, &state).await {
        let (msg, err_code) = match code {
            StatusCode::UNAUTHORIZED => ("unauthorized", "UNAUTHORIZED"),
            StatusCode::FORBIDDEN => ("vault locked", "VAULT_LOCKED"),
            _ => ("internal error", "INTERNAL_ERROR"),
        };
        return err_json(code, msg, err_code).into_response();
    }

    let path = std::path::PathBuf::from(&body.path);
    match crate::share::import_package(&path, &body.passphrase, &state.vault).await {
        Ok(names) => {
            let count = names.len();
            (
                StatusCode::OK,
                Json(ShareImportResponse {
                    imported: count,
                    item_names: names,
                }),
            )
                .into_response()
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), "INTERNAL_ERROR")
            .into_response(),
    }
}

// ─── Función pública de arranque ──────────────────────────────────────────────

pub async fn start_server(vault: SharedState, app_data_dir: PathBuf) {
    let api_state = Arc::new(ApiState {
        vault,
        session_token: Arc::new(Mutex::new(None)),
        token_expires: Arc::new(Mutex::new(None)),
        unlock_rate: Mutex::new(RateLimitState {
            attempts: 0,
            window_start: Instant::now(),
        }),
        share: Arc::new(ShareState::new()),
    });

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/unlock", post(handle_unlock))
        .route("/fill", post(handle_fill))
        .route("/items", get(handle_list_items))
        .route("/items", post(handle_create_item))
        .route("/items/:id", get(handle_get_item))
        .route("/items/:id", put(handle_update_item))
        .route("/items/:id", delete(handle_delete_item))
        .route("/items/:id/reveal", post(handle_reveal_item))
        .route("/categories", get(handle_list_categories))
        .route("/categories", post(handle_create_category))
        .route("/categories/:id", put(handle_update_category))
        .route("/categories/:id", delete(handle_delete_category))
        .route("/commands", get(handle_list_commands))
        .route("/commands/:id", get(handle_get_command))
        .route("/settings", get(handle_get_settings))
        .route("/settings", put(handle_put_settings))
        .route("/share/listen", post(handle_share_listen))
        .route("/share/connect", post(handle_share_connect))
        .route("/share/confirm", post(handle_share_confirm))
        .route("/share/status", get(handle_share_status))
        .route("/share/session", delete(handle_share_cancel))
        .route("/share/export", post(handle_share_export))
        .route("/share/import", post(handle_share_import))
        .with_state(api_state)
        .layer(middleware::from_fn(cors_guard));

    const ADDR: &str = "127.0.0.1:47821";

    // Ensure a valid self-signed TLS certificate is present (generated on first
    // launch, regenerated if within 30 days of expiry).
    let tls_config = match tls::ensure_tls_config(&app_data_dir).await {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[api] Failed to initialise TLS certificate: {e}");
            return;
        }
    };

    let addr: std::net::SocketAddr = match ADDR.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[api] Invalid bind address {ADDR}: {e}");
            return;
        }
    };

    eprintln!("[api] Listening on https://{ADDR} (TLS)");

    if let Err(e) = axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
    {
        eprintln!("[api] REST server error: {e}");
    }
}
