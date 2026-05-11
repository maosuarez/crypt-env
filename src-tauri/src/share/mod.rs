pub mod crypto;
pub mod lan;
pub mod package;
pub mod protocol;

use rand::RngCore;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use package::PlainItem;

// ─── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ShareError {
    /// Vault-layer error (DB or crypto)
    Vault(String),
    /// Network I/O error
    Io(String),
    /// Wire protocol error (bad message, malformed data)
    Protocol(String),
    /// mDNS discovery error
    Discovery(String),
    /// Cryptographic error
    Crypto(String),
    /// Operation timed out
    Timeout,
    /// Remote peer sent an error
    Remote(String),
    /// Session is in an unexpected state
    InvalidState(String),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareError::Vault(e) => write!(f, "vault error: {e}"),
            ShareError::Io(e) => write!(f, "I/O error: {e}"),
            ShareError::Protocol(e) => write!(f, "protocol error: {e}"),
            ShareError::Discovery(e) => write!(f, "discovery error: {e}"),
            ShareError::Crypto(e) => write!(f, "crypto error: {e}"),
            ShareError::Timeout => write!(f, "operation timed out"),
            ShareError::Remote(e) => write!(f, "peer error: {e}"),
            ShareError::InvalidState(e) => write!(f, "invalid session state: {e}"),
        }
    }
}

// ─── Session state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ShareSessionState {
    Listening,
    Connecting,
    AwaitingFingerprint,
    Active,
    Done,
    Failed(String),
    Cancelled,
}

impl ShareSessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShareSessionState::Listening => "listening",
            ShareSessionState::Connecting => "connecting",
            ShareSessionState::AwaitingFingerprint => "awaiting_fingerprint",
            ShareSessionState::Active => "active",
            ShareSessionState::Done => "done",
            ShareSessionState::Failed(_) => "failed",
            ShareSessionState::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShareDirection {
    Sending,
    Receiving,
}

impl ShareDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShareDirection::Sending => "sending",
            ShareDirection::Receiving => "receiving",
        }
    }
}

/// Ephemeral keypair that zeroizes the secret on drop.
pub struct EphemeralKeypair {
    pub secret: StaticSecret,
    pub public: PublicKey,
}

impl EphemeralKeypair {
    pub fn generate() -> Self {
        let (secret, public) = crypto::generate_keypair();
        EphemeralKeypair { secret, public }
    }
}

impl Drop for EphemeralKeypair {
    fn drop(&mut self) {
        // StaticSecret implements Zeroize internally; calling drop here is sufficient.
        // We rely on the x25519-dalek guarantee that StaticSecret zeroes on drop.
    }
}

pub struct ShareSession {
    pub id: Uuid,
    pub pairing_code: String,
    pub our_pubkey: [u8; 32],
    pub peer_pubkey: Option<[u8; 32]>,
    /// Shared key wrapped in Zeroizing — cleared on drop.
    pub shared_key: Option<Zeroizing<[u8; 32]>>,
    pub state: ShareSessionState,
    pub created_at: Instant,
    pub last_active: Instant,
    /// Item IDs to send (sender side only).
    pub items_to_send: Vec<i64>,
    pub direction: ShareDirection,
    /// Hex fingerprint computed after ECDH.
    pub fingerprint: Option<String>,
    /// Received plain items (receiver side, after Done).
    pub received_items: Vec<PlainItem>,
    /// Names of items received (available after Done for the API response).
    pub received_names: Vec<String>,
}

/// Top-level share state — one optional active session at a time.
/// The `Option` is intentional: a new session replaces or rejects while one is active.
pub struct ShareState {
    pub session: Arc<Mutex<Option<ShareSession>>>,
}

impl ShareState {
    pub fn new() -> Self {
        ShareState {
            session: Arc::new(Mutex::new(None)),
        }
    }
}

// ─── Pairing code generation ──────────────────────────────────────────────────

/// Generate a cryptographically random 6-digit numeric pairing code.
pub fn generate_pairing_code() -> String {
    let mut buf = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut buf);
    let n = u32::from_le_bytes(buf) % 1_000_000;
    format!("{:06}", n)
}

// ─── Public functions ─────────────────────────────────────────────────────────

/// Start a sender session: generate a session, start the mDNS listener in a
/// background Tokio task, and return the pairing code.
///
/// The background task:
/// 1. Accepts one TCP connection.
/// 2. Runs the ECDH handshake.
/// 3. Sets state to AwaitingFingerprint with the fingerprint.
/// 4. Waits for the fingerprint to be confirmed (state → Active) or cancelled.
/// 5. If Active: sends items, sets state Done.
/// 6. If not: sends Error, sets state Cancelled or Failed.
///
/// The vault key and DB are passed in so the task can decrypt items independently.
pub async fn start_listen_session(
    share_state: Arc<ShareState>,
    item_ids: Vec<i64>,
    vault_key: [u8; 32],
    db: Arc<Mutex<crate::vault::VaultState>>,
) -> Result<String, ShareError> {
    let keypair = EphemeralKeypair::generate();
    let our_pub = *keypair.public.as_bytes();
    let pairing_code = generate_pairing_code();

    let session = ShareSession {
        id: Uuid::new_v4(),
        pairing_code: pairing_code.clone(),
        our_pubkey: our_pub,
        peer_pubkey: None,
        shared_key: None,
        state: ShareSessionState::Listening,
        created_at: Instant::now(),
        last_active: Instant::now(),
        items_to_send: item_ids.clone(),
        direction: ShareDirection::Sending,
        fingerprint: None,
        received_items: Vec::new(),
        received_names: Vec::new(),
    };

    {
        let mut guard = share_state.session.lock().await;
        *guard = Some(session);
    }

    let state_clone = share_state.clone();
    let code_clone = pairing_code.clone();
    let item_ids_clone = item_ids.clone();

    tokio::spawn(async move {
        let result = run_send_background(
            state_clone.clone(),
            keypair,
            code_clone,
            item_ids_clone,
            vault_key,
            db,
        )
        .await;

        if let Err(e) = result {
            let mut guard = state_clone.session.lock().await;
            if let Some(ref mut s) = *guard {
                s.state = ShareSessionState::Failed(e.to_string());
            }
        }
    });

    Ok(pairing_code)
}

async fn run_send_background(
    share_state: Arc<ShareState>,
    keypair: EphemeralKeypair,
    pairing_code: String,
    item_ids: Vec<i64>,
    vault_key: [u8; 32],
    db_state: Arc<Mutex<crate::vault::VaultState>>,
) -> Result<(), ShareError> {
    use std::time::Duration;

    // Start mDNS listener — keep daemon alive in this scope
    let (listener, _port, _mdns_daemon) = lan::start_listener(&pairing_code)?;

    let accept_timeout = Duration::from_secs(300); // 5 minutes
    let start = Instant::now();

    // Set the listener to non-blocking so we can poll with cancellation checks.
    listener
        .set_nonblocking(true)
        .map_err(|e| ShareError::Io(e.to_string()))?;

    // Poll for accept with periodic cancellation checks
    let mut stream_opt: Option<std::net::TcpStream> = None;
    loop {
        if start.elapsed() >= accept_timeout {
            let mut guard = share_state.session.lock().await;
            if let Some(ref mut s) = *guard {
                s.state = ShareSessionState::Failed("pairing code expired".into());
            }
            return Err(ShareError::Timeout);
        }

        // Check if cancelled
        {
            let guard = share_state.session.lock().await;
            if let Some(ref s) = *guard {
                if s.state == ShareSessionState::Cancelled {
                    return Ok(());
                }
            }
        }

        match listener.accept() {
            Ok((s, _)) => {
                stream_opt = Some(s);
                break;
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            Err(e) => return Err(ShareError::Io(format!("accept: {e}"))),
        }
    }

    let mut stream = stream_opt.expect("stream set in loop above");

    // ECDH handshake
    let our_pub = keypair.public;
    let (stream_after, shared_key, fingerprint) =
        tokio::task::spawn_blocking(move || {
            lan::sender_handshake(stream, &keypair.secret, &our_pub)
        })
        .await
        .map_err(|e| ShareError::Io(e.to_string()))??;

    stream = stream_after;

    // Update session: AwaitingFingerprint
    {
        let mut guard = share_state.session.lock().await;
        if let Some(ref mut s) = *guard {
            s.fingerprint = Some(fingerprint.clone());
            s.state = ShareSessionState::AwaitingFingerprint;
            s.last_active = Instant::now();
        }
    }

    // Wait for user to confirm fingerprint (up to 5 minutes)
    let confirm_deadline = Instant::now() + Duration::from_secs(300);
    let confirmed;
    loop {
        if Instant::now() >= confirm_deadline {
            return Err(ShareError::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;

        let guard = share_state.session.lock().await;
        if let Some(ref s) = *guard {
            match &s.state {
                ShareSessionState::Active => {
                    confirmed = true;
                    break;
                }
                ShareSessionState::Cancelled => {
                    confirmed = false;
                    break;
                }
                ShareSessionState::Failed(_) => return Ok(()),
                _ => continue,
            }
        }
    }

    if !confirmed {
        // Notify peer — use the local shared_key we derived during handshake
        let _ = tokio::task::spawn_blocking(move || {
            lan::sender_reject(stream, &shared_key, "user rejected fingerprint")
        })
        .await;
        return Ok(());
    }

    // Decrypt items for sharing
    let db_items = {
        let vault_guard = db_state.lock().await;
        let raw = vault_guard
            .db
            .list_items()
            .await
            .map_err(|e| ShareError::Vault(e))?;
        raw
    };

    let mut plain_items = Vec::new();
    for (id, _, data, _) in &db_items {
        if !item_ids.contains(id) {
            continue;
        }
        let plaintext =
            crate::crypto::decrypt(&vault_key, data).map_err(|e| ShareError::Vault(e))?;
        let item: crate::vault::VaultItem =
            serde_json::from_slice(&plaintext).map_err(|e| ShareError::Protocol(e.to_string()))?;
        plain_items.push(PlainItem {
            item_type: item.item_type,
            name: item.name.unwrap_or_default(),
            value: item.value,
            username: item.username,
            password: item.password,
            url: item.url,
            notes: item.notes,
            category: item.categories.into_iter().next(),
            command: item.command,
        });
    }

    let item_count = plain_items.len();

    // Send items
    let shared_key_clone = shared_key.clone();
    let result = tokio::task::spawn_blocking(move || {
        lan::sender_send_items(stream, &shared_key_clone, plain_items)
    })
    .await
    .map_err(|e| ShareError::Io(e.to_string()))??;

    // Log and mark Done
    {
        let vault_guard = db_state.lock().await;
        let _ = vault_guard
            .db
            .log_share(
                "lan",
                "sending",
                &item_ids,
                Some(&fingerprint),
            )
            .await;
    }

    {
        let mut guard = share_state.session.lock().await;
        if let Some(ref mut s) = *guard {
            s.state = ShareSessionState::Done;
            s.last_active = Instant::now();
        }
    }

    let _ = result; // ack count — not used for now
    let _ = item_count;
    Ok(())
}

/// Start a receiver session: connect to a LAN peer, perform ECDH, and set state
/// to AwaitingFingerprint. Returns the fingerprint.
pub async fn connect_to_peer(
    share_state: Arc<ShareState>,
    pairing_code: String,
    vault_key: [u8; 32],
    db: Arc<Mutex<crate::vault::VaultState>>,
) -> Result<String, ShareError> {
    let keypair = EphemeralKeypair::generate();
    let our_pub = *keypair.public.as_bytes();

    let session = ShareSession {
        id: Uuid::new_v4(),
        pairing_code: pairing_code.clone(),
        our_pubkey: our_pub,
        peer_pubkey: None,
        shared_key: None,
        state: ShareSessionState::Connecting,
        created_at: Instant::now(),
        last_active: Instant::now(),
        items_to_send: Vec::new(),
        direction: ShareDirection::Receiving,
        fingerprint: None,
        received_items: Vec::new(),
        received_names: Vec::new(),
    };

    {
        let mut guard = share_state.session.lock().await;
        *guard = Some(session);
    }

    // Connect to peer via mDNS (30s timeout)
    let code_for_connect = pairing_code.clone();
    let stream = tokio::task::spawn_blocking(move || {
        lan::connect_to_peer(&code_for_connect, 30)
    })
    .await
    .map_err(|e| ShareError::Io(e.to_string()))??;

    let our_pub_key = keypair.public;
    let (stream_after, shared_key, fingerprint) =
        tokio::task::spawn_blocking(move || {
            lan::receiver_handshake(stream, &keypair.secret, &our_pub_key)
        })
        .await
        .map_err(|e| ShareError::Io(e.to_string()))??;

    let fp_clone = fingerprint.clone();

    // Store shared key + set AwaitingFingerprint
    {
        let mut guard = share_state.session.lock().await;
        if let Some(ref mut s) = *guard {
            s.shared_key = Some(shared_key.clone());
            s.fingerprint = Some(fingerprint.clone());
            s.state = ShareSessionState::AwaitingFingerprint;
            s.last_active = Instant::now();
        }
    }

    // Background task: wait for confirmation, then receive items
    let state_clone = share_state.clone();
    tokio::spawn(async move {
        let result = run_receive_background(
            state_clone.clone(),
            stream_after,
            shared_key,
            fingerprint.clone(),
            vault_key,
            db,
        )
        .await;
        if let Err(e) = result {
            let mut guard = state_clone.session.lock().await;
            if let Some(ref mut s) = *guard {
                s.state = ShareSessionState::Failed(e.to_string());
            }
        }
    });

    Ok(fp_clone)
}

async fn run_receive_background(
    share_state: Arc<ShareState>,
    stream: std::net::TcpStream,
    shared_key: Zeroizing<[u8; 32]>,
    fingerprint: String,
    vault_key: [u8; 32],
    db_state: Arc<Mutex<crate::vault::VaultState>>,
) -> Result<(), ShareError> {
    use std::time::Duration;

    // Wait for user to confirm fingerprint (up to 5 minutes)
    let confirm_deadline = Instant::now() + Duration::from_secs(300);
    let confirmed;
    loop {
        if Instant::now() >= confirm_deadline {
            return Err(ShareError::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;

        let guard = share_state.session.lock().await;
        if let Some(ref s) = *guard {
            match &s.state {
                ShareSessionState::Active => {
                    confirmed = true;
                    break;
                }
                ShareSessionState::Cancelled => {
                    confirmed = false;
                    break;
                }
                ShareSessionState::Failed(_) => return Ok(()),
                _ => continue,
            }
        }
    }

    if !confirmed {
        return Ok(());
    }

    // Receive items
    let shared_key_clone = shared_key.clone();
    let items = tokio::task::spawn_blocking(move || {
        lan::receiver_receive_items(stream, &shared_key_clone)
    })
    .await
    .map_err(|e| ShareError::Io(e.to_string()))??;

    let item_count = items.len();
    let item_ids: Vec<i64> = Vec::new(); // receiver doesn't know IDs before import

    // Import items into vault and log — acquire the mutex once for both operations
    let names = {
        let vault_guard = db_state.lock().await;
        let imported = import_plain_items_into_vault(&items, &vault_key, &vault_guard.db).await?;
        let _ = vault_guard
            .db
            .log_share("lan", "receiving", &item_ids, Some(&fingerprint))
            .await;
        imported
    };

    {
        let mut guard = share_state.session.lock().await;
        if let Some(ref mut s) = *guard {
            s.received_names = names.clone();
            s.state = ShareSessionState::Done;
            s.last_active = Instant::now();
        }
    }

    let _ = item_count;
    Ok(())
}

async fn import_plain_items_into_vault(
    items: &[PlainItem],
    vault_key: &[u8; 32],
    db: &crate::db::VaultDb,
) -> Result<Vec<String>, ShareError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let mut names = Vec::new();

    for plain in items {
        let vault_item = crate::vault::VaultItem {
            id: 0,
            item_type: plain.item_type.clone(),
            name: Some(plain.name.clone()),
            value: plain.value.clone(),
            username: plain.username.clone(),
            password: plain.password.clone(),
            url: plain.url.clone(),
            notes: plain.notes.clone(),
            title: None,
            description: None,
            command: plain.command.clone(),
            shell: None,
            content: None,
            categories: plain.category.iter().cloned().collect(),
            created: now_ts.clone(),
        };

        let json = serde_json::to_vec(&vault_item)
            .map_err(|e| ShareError::Protocol(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(vault_key, &json)
            .map_err(|e| ShareError::Vault(e))?;

        db.upsert_item(0, &vault_item.item_type, &encrypted, &vault_item.created)
            .await
            .map_err(|e| ShareError::Vault(e))?;

        names.push(plain.name.clone());
    }

    Ok(names)
}

/// Confirm (or reject) the fingerprint for the active session.
/// Moving to Active state unblocks the background task to proceed with the transfer.
pub async fn confirm_fingerprint(
    share_state: &Arc<ShareState>,
    confirmed: bool,
) -> Result<(), ShareError> {
    let mut guard = share_state.session.lock().await;
    match guard.as_mut() {
        None => Err(ShareError::InvalidState("no active session".into())),
        Some(s) => {
            if s.state != ShareSessionState::AwaitingFingerprint {
                return Err(ShareError::InvalidState(format!(
                    "session is in state {:?}, not AwaitingFingerprint",
                    s.state
                )));
            }
            s.state = if confirmed {
                ShareSessionState::Active
            } else {
                ShareSessionState::Cancelled
            };
            s.last_active = Instant::now();
            Ok(())
        }
    }
}

/// Cancel the active session immediately.
pub async fn cancel_session(share_state: &Arc<ShareState>) -> Result<(), ShareError> {
    let mut guard = share_state.session.lock().await;
    match guard.as_mut() {
        None => Err(ShareError::InvalidState("no active session".into())),
        Some(s) => {
            s.state = ShareSessionState::Cancelled;
            s.last_active = Instant::now();
            Ok(())
        }
    }
}

/// Export selected items as an encrypted package file.
/// Returns (passphrase, path).
pub async fn export_package(
    item_ids: &[i64],
    output_path: &std::path::Path,
    vault_state: &Arc<Mutex<crate::vault::VaultState>>,
) -> Result<String, ShareError> {
    let passphrase = crypto::generate_passphrase();

    let (vault_key, raw_items) = {
        let guard = vault_state.lock().await;
        let k = guard
            .key
            .as_ref()
            .ok_or_else(|| ShareError::Vault("vault is locked".into()))?;
        let key: [u8; 32] = **k;
        let raw = guard
            .db
            .list_items()
            .await
            .map_err(|e| ShareError::Vault(e))?;
        (key, raw)
    };

    let mut plain_items = Vec::new();
    for (id, _, data, _) in &raw_items {
        if !item_ids.contains(id) {
            continue;
        }
        let plaintext = crate::crypto::decrypt(&vault_key, data)
            .map_err(|e| ShareError::Vault(e))?;
        let item: crate::vault::VaultItem = serde_json::from_slice(&plaintext)
            .map_err(|e| ShareError::Protocol(e.to_string()))?;
        plain_items.push(PlainItem {
            item_type: item.item_type,
            name: item.name.unwrap_or_default(),
            value: item.value,
            username: item.username,
            password: item.password,
            url: item.url,
            notes: item.notes,
            category: item.categories.into_iter().next(),
            command: item.command,
        });
    }

    // Run the file write on a blocking thread (file I/O)
    let pass_clone = passphrase.clone();
    let path_clone = output_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        package::export_package(&plain_items, &path_clone, &pass_clone)
    })
    .await
    .map_err(|e| ShareError::Io(e.to_string()))??;

    // Log
    {
        let guard = vault_state.lock().await;
        let _ = guard.db.log_share("package", "sending", item_ids, None).await;
    }

    Ok(passphrase)
}

/// Import an encrypted package file into the vault.
/// Returns names of imported items.
pub async fn import_package(
    path: &std::path::Path,
    passphrase: &str,
    vault_state: &Arc<Mutex<crate::vault::VaultState>>,
) -> Result<Vec<String>, ShareError> {
    let path_clone = path.to_path_buf();
    let pass_clone = passphrase.to_string();

    // Decrypt package on a blocking thread
    let plain_items = tokio::task::spawn_blocking(move || {
        package::import_package(&path_clone, &pass_clone)
    })
    .await
    .map_err(|e| ShareError::Io(e.to_string()))??;

    // Import into vault
    let vault_key: [u8; 32] = {
        let guard = vault_state.lock().await;
        let k = guard
            .key
            .as_ref()
            .ok_or_else(|| ShareError::Vault("vault is locked".into()))?;
        **k
    };

    let guard = vault_state.lock().await;
    let names = import_plain_items_into_vault(&plain_items, &vault_key, &guard.db).await?;

    // Log
    let _ = guard.db.log_share("package", "receiving", &[], None).await;

    Ok(names)
}
