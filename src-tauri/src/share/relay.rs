use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::package::PlainItem;
use super::ShareError;
use crate::share::crypto::{decrypt_message, encrypt_message};

// ─── Relay session structs ────────────────────────────────────────────────────

#[derive(Serialize)]
struct RelayInsert<'a> {
    code: &'a str,
    payload: &'a str,
    expires_at: &'a str,
}

#[derive(Deserialize)]
struct RelayRow {
    payload: String,
}

// ─── Key derivation ───────────────────────────────────────────────────────────

/// Derive a 32-byte relay encryption key from (code, passphrase) deterministically.
/// salt = SHA-256(code) so both sides can reproduce without extra transmission.
pub fn derive_relay_key(code: &str, passphrase: &str) -> Result<[u8; 32], ShareError> {
    let salt: [u8; 32] = Sha256::digest(code.as_bytes()).into();
    let params = Params::new(32768, 2, 2, Some(32))
        .map_err(|e| ShareError::Crypto(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
        .map_err(|e| ShareError::Crypto(e.to_string()))?;
    Ok(key)
}

// ─── Payload encryption ───────────────────────────────────────────────────────

pub fn encrypt_items(items: &[PlainItem], key: &[u8; 32]) -> Result<String, ShareError> {
    let json = serde_json::to_vec(items).map_err(|e| ShareError::Protocol(e.to_string()))?;
    let ciphertext = encrypt_message(key, &json);
    Ok(B64.encode(&ciphertext))
}

pub fn decrypt_payload(payload: &str, key: &[u8; 32]) -> Result<Vec<PlainItem>, ShareError> {
    let ciphertext = B64
        .decode(payload)
        .map_err(|e| ShareError::Protocol(format!("base64 decode: {e}")))?;
    let plaintext = decrypt_message(key, &ciphertext)?;
    serde_json::from_slice(&plaintext).map_err(|e| ShareError::Protocol(e.to_string()))
}

// ─── Complete-workspace bundle (template + values) ────────────────────────────
// A workspace bundle carries the workspace definition AND the decrypted values of
// every referenced vault item, so a teammate can reconstruct a ready-to-inject
// workspace in one step. It rides on the same relay encryption as plain items.

/// One variable in a shared workspace. Either references a bundled item (by name)
/// or carries an inline literal value — never both.
#[derive(Serialize, Deserialize, Clone)]
pub struct WorkspaceBundleVar {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
}

/// A complete, self-contained workspace ready to import.
#[derive(Serialize, Deserialize, Clone)]
pub struct WorkspaceBundle {
    /// Discriminator so a workspace payload is never mistaken for a bare item list.
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub template: String,
    pub vars: Vec<WorkspaceBundleVar>,
    /// Decrypted values for the item-backed vars. Matched to vars by `name`.
    pub items: Vec<PlainItem>,
}

impl WorkspaceBundle {
    pub const KIND: &'static str = "workspace";
}

pub fn encrypt_workspace(bundle: &WorkspaceBundle, key: &[u8; 32]) -> Result<String, ShareError> {
    let json = serde_json::to_vec(bundle).map_err(|e| ShareError::Protocol(e.to_string()))?;
    let ciphertext = encrypt_message(key, &json);
    Ok(B64.encode(&ciphertext))
}

pub fn decrypt_workspace(payload: &str, key: &[u8; 32]) -> Result<WorkspaceBundle, ShareError> {
    let ciphertext = B64
        .decode(payload)
        .map_err(|e| ShareError::Protocol(format!("base64 decode: {e}")))?;
    let plaintext = decrypt_message(key, &ciphertext)?;
    let bundle: WorkspaceBundle =
        serde_json::from_slice(&plaintext).map_err(|e| ShareError::Protocol(e.to_string()))?;
    if bundle.kind != WorkspaceBundle::KIND {
        return Err(ShareError::Protocol(
            "this code is not a workspace package (use the items receive flow instead)".into(),
        ));
    }
    Ok(bundle)
}

// ─── Code generation ──────────────────────────────────────────────────────────

pub fn generate_share_code() -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 8];
    rng.fill_bytes(&mut buf);
    let chars: String = buf
        .iter()
        .map(|b| ALPHA[(*b as usize) % ALPHA.len()] as char)
        .collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

// ─── Supabase relay calls (blocking reqwest) ──────────────────────────────────

pub fn relay_upload(
    supabase_url: &str,
    anon_key: &str,
    code: &str,
    payload: &str,
) -> Result<(), ShareError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let expires_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 86400; // 24 hours
    let expires_at = format_iso8601(expires_secs);

    let body = RelayInsert {
        code,
        payload,
        expires_at: &expires_at,
    };

    let client = reqwest::blocking::Client::new();
    let res = client
        .post(format!("{}/rest/v1/relay_packages", supabase_url.trim_end_matches('/')))
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", anon_key))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()
        .map_err(|e| ShareError::Io(e.to_string()))?;

    if !res.status().is_success() {
        let status = res.status().as_u16();
        let body = res.text().unwrap_or_default();
        return Err(ShareError::Io(format!("relay upload failed ({status}): {body}")));
    }
    Ok(())
}

pub fn relay_download(
    supabase_url: &str,
    anon_key: &str,
    code: &str,
) -> Result<String, ShareError> {
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "{}/rest/v1/relay_packages?code=eq.{}&retrieved=eq.false&select=payload",
        supabase_url.trim_end_matches('/'),
        code,
    );
    let res = client
        .get(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", anon_key))
        .header("Accept", "application/json")
        .send()
        .map_err(|e| ShareError::Io(e.to_string()))?;

    if !res.status().is_success() {
        let status = res.status().as_u16();
        return Err(ShareError::Io(format!("relay download failed ({status})")));
    }

    let rows: Vec<RelayRow> = res
        .json()
        .map_err(|e| ShareError::Protocol(e.to_string()))?;

    rows.into_iter()
        .next()
        .map(|r| r.payload)
        .ok_or(ShareError::Remote("code not found or already used".into()))
}

pub fn relay_delete(
    supabase_url: &str,
    anon_key: &str,
    code: &str,
) -> Result<(), ShareError> {
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "{}/rest/v1/relay_packages?code=eq.{}",
        supabase_url.trim_end_matches('/'),
        code,
    );
    let _ = client
        .delete(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", anon_key))
        .send()
        .map_err(|e| ShareError::Io(e.to_string()))?;
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn format_iso8601(unix_secs: u64) -> String {
    // Simple ISO-8601 UTC formatter without external deps
    let s = unix_secs;
    let secs = s % 60;
    let mins = (s / 60) % 60;
    let hours = (s / 3600) % 24;
    let days = s / 86400;
    // Days since epoch → year/month/day (Gregorian, approximation sufficient for +24h TTL)
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Gregorian calendar from Unix epoch (1970-01-01)
    let mut d = days as i64;
    let mut y = 1970i64;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if d < dy {
            break;
        }
        d -= dy;
        y += 1;
    }
    let months = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1u64;
    for dm in &months {
        if d < *dm {
            break;
        }
        d -= dm;
        m += 1;
    }
    (y as u64, m, d as u64 + 1)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
