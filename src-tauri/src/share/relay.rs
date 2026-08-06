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

// ─── Project bundle (structure + values for N environments at once) ──────────
// A project bundle carries an entire project's definition — its environments
// and the decrypted values of every item they reference — so a teammate can
// reconstruct a ready-to-inject multi-environment project in one step. Items
// are hoisted to the bundle root and deduped by name, referenced from each
// environment's vars by name (see docs/plans/issue-4 D1): this is what lets
// "the same item linked into 3 environments" be told apart from "3 items
// that happen to share a name" on receive, which the old per-environment
// `WorkspaceBundle` shape (removed) could not distinguish.

/// One variable in a shared environment — always references a bundled item
/// by name. Unlike the legacy workspace format, there is no inline literal:
/// `environment_vars.item_id` is mandatory post-migration, so every var this
/// bundle can even represent already resolves to a real item (D3).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectBundleVar {
    pub key: String,
    pub item_name: String,
}

/// One environment's shape, values-free of anything machine-specific.
/// Deliberately has no `paths` field (D6) — absolute filesystem paths from
/// the sender's machine are meaningless (and identity-leaking) on the
/// receiver's, so the field does not exist in the wire type rather than
/// being included-but-ignored.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnvironmentBundle {
    pub name: String,
    pub is_default: bool,
    pub vars: Vec<ProjectBundleVar>,
}

/// A complete, self-contained project ready to import: N environments plus
/// the deduped set of items they reference.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectBundle {
    /// Discriminator so a project payload is never mistaken for a bare item list.
    pub kind: String,
    /// Format version, checked on decrypt (D2) — bumping it is free now and
    /// impossible to retrofit later, so it's checked from day one even
    /// though only version 1 currently exists.
    pub version: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub template: String,
    pub environments: Vec<EnvironmentBundle>,
    /// Decrypted values, deduped by name. Matched to vars by `item_name`.
    pub items: Vec<PlainItem>,
}

impl ProjectBundle {
    pub const KIND: &'static str = "project";
    pub const VERSION: u32 = 1;
}

pub fn encrypt_project(bundle: &ProjectBundle, key: &[u8; 32]) -> Result<String, ShareError> {
    let json = serde_json::to_vec(bundle).map_err(|e| ShareError::Protocol(e.to_string()))?;
    let ciphertext = encrypt_message(key, &json);
    Ok(B64.encode(&ciphertext))
}

/// Decrypts and validates a project bundle: rejects a wrong `kind` (e.g. a
/// payload produced by `encrypt_items`) and an unknown `version` before
/// returning, so a format change never has to repeat this discriminator dance.
pub fn decrypt_project(payload: &str, key: &[u8; 32]) -> Result<ProjectBundle, ShareError> {
    let ciphertext = B64
        .decode(payload)
        .map_err(|e| ShareError::Protocol(format!("base64 decode: {e}")))?;
    let plaintext = decrypt_message(key, &ciphertext)?;
    let bundle: ProjectBundle =
        serde_json::from_slice(&plaintext).map_err(|e| ShareError::Protocol(e.to_string()))?;
    if bundle.kind != ProjectBundle::KIND {
        return Err(ShareError::Protocol(
            "this code is not a project package (use the items receive flow instead)".into(),
        ));
    }
    if bundle.version != ProjectBundle::VERSION {
        return Err(ShareError::Protocol(format!(
            "this package was created by a newer version of CryptEnv (format v{}); update to receive it",
            bundle.version
        )));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bundle() -> ProjectBundle {
        ProjectBundle {
            kind: ProjectBundle::KIND.to_string(),
            version: ProjectBundle::VERSION,
            name: "MyApp".to_string(),
            description: Some("a sample project".to_string()),
            template: "node".to_string(),
            environments: vec![
                EnvironmentBundle {
                    name: "local".to_string(),
                    is_default: true,
                    vars: vec![
                        ProjectBundleVar { key: "DB_HOST".to_string(), item_name: "db-host".to_string() },
                        ProjectBundleVar { key: "DB_PASSWORD".to_string(), item_name: "db-password".to_string() },
                    ],
                },
                EnvironmentBundle {
                    name: "production".to_string(),
                    is_default: false,
                    vars: vec![ProjectBundleVar {
                        key: "DB_HOST".to_string(),
                        item_name: "db-host".to_string(),
                    }],
                },
            ],
            items: vec![
                PlainItem {
                    item_type: "secret".to_string(),
                    name: "db-host".to_string(),
                    value: Some("localhost".to_string()),
                    username: None,
                    password: None,
                    url: None,
                    notes: None,
                    category: None,
                    command: None,
                },
                PlainItem {
                    item_type: "secret".to_string(),
                    name: "db-password".to_string(),
                    value: Some("hunter2".to_string()),
                    username: None,
                    password: None,
                    url: None,
                    notes: None,
                    category: None,
                    command: None,
                },
            ],
        }
    }

    #[test]
    fn project_bundle_roundtrip_preserves_structure() {
        let bundle = sample_bundle();
        let key = derive_relay_key("TEST-CODE", "correct horse battery staple").unwrap();

        let payload = encrypt_project(&bundle, &key).unwrap();
        let decrypted = decrypt_project(&payload, &key).unwrap();

        assert_eq!(decrypted.environments.len(), bundle.environments.len());
        for (a, b) in decrypted.environments.iter().zip(bundle.environments.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.is_default, b.is_default);
            let a_keys: Vec<&str> = a.vars.iter().map(|v| v.key.as_str()).collect();
            let b_keys: Vec<&str> = b.vars.iter().map(|v| v.key.as_str()).collect();
            assert_eq!(a_keys, b_keys);
        }
        let mut a_names: Vec<&str> = decrypted.items.iter().map(|i| i.name.as_str()).collect();
        let mut b_names: Vec<&str> = bundle.items.iter().map(|i| i.name.as_str()).collect();
        a_names.sort();
        b_names.sort();
        assert_eq!(a_names, b_names);
    }

    #[test]
    fn decrypt_project_rejects_items_payload() {
        let key = derive_relay_key("TEST-CODE", "correct horse battery staple").unwrap();
        let items = vec![PlainItem {
            item_type: "secret".to_string(),
            name: "loose-item".to_string(),
            value: Some("x".to_string()),
            username: None,
            password: None,
            url: None,
            notes: None,
            category: None,
            command: None,
        }];
        let payload = encrypt_items(&items, &key).unwrap();

        let result = decrypt_project(&payload, &key);
        match result {
            Err(ShareError::Protocol(_)) => {}
            other => panic!("expected ShareError::Protocol, got {other:?}"),
        }
    }

    #[test]
    fn decrypt_project_rejects_unknown_version() {
        let key = derive_relay_key("TEST-CODE", "correct horse battery staple").unwrap();
        let mut bundle = sample_bundle();
        bundle.version = 99;
        let payload = encrypt_project(&bundle, &key).unwrap();

        let err = decrypt_project(&payload, &key).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("v99"), "message should name the unsupported version, got: {msg}");
    }

    #[test]
    fn decrypt_project_rejects_wrong_passphrase() {
        let bundle = sample_bundle();
        let key = derive_relay_key("TEST-CODE", "correct horse battery staple").unwrap();
        let payload = encrypt_project(&bundle, &key).unwrap();

        let wrong_key = derive_relay_key("TEST-CODE", "totally different passphrase").unwrap();
        let result = decrypt_project(&payload, &wrong_key);
        match result {
            Err(ShareError::Crypto(_)) => {}
            other => panic!("expected ShareError::Crypto, got {other:?}"),
        }
    }
}
