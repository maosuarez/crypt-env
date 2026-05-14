use mdns_sd::{ServiceDaemon, ServiceInfo};
use sha2::Digest;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::crypto::{compute_fingerprint, derive_shared_key};
use super::package::PlainItem;
use super::protocol::{recv_encrypted, recv_plain, send_encrypted, send_plain, ShareMessage};
use super::ShareError;

const SERVICE_TYPE: &str = "_cryptenv._tcp.local.";

// ─── Pairing code hashing ─────────────────────────────────────────────────────

/// Compute the first 8 hex characters of SHA-256(pairing_code).
/// This hash is broadcast via mDNS TXT so the pairing code itself is never
/// transmitted in plaintext over the network.
fn hash_pairing_code(code: &str) -> String {
    let digest = sha2::Sha256::digest(code.as_bytes());
    digest[..4].iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── Listener ─────────────────────────────────────────────────────────────────

/// Bind a TCP listener on a random port and register an mDNS service so that
/// peers can discover this session using only the pairing code.
///
/// Returns `(TcpListener, port, ServiceDaemon)`.
/// The caller must keep the `ServiceDaemon` alive until the session ends —
/// dropping it unregisters the mDNS service.
pub fn start_listener(
    pairing_code: &str,
) -> Result<(TcpListener, u16, ServiceDaemon), ShareError> {
    // Bind to a random port on all interfaces so peers on the LAN can connect.
    let listener = TcpListener::bind("0.0.0.0:0")
        .map_err(|e| ShareError::Io(format!("bind TCP listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| ShareError::Io(e.to_string()))?
        .port();

    // Register mDNS service with the hashed pairing code in the TXT record.
    let pc_hash = hash_pairing_code(pairing_code);
    let mdns = ServiceDaemon::new()
        .map_err(|e| ShareError::Discovery(format!("mdns daemon: {e}")))?;

    let hostname = format!(
        "cryptenv-{}.local.",
        &pc_hash[..8.min(pc_hash.len())]
    );

    let mut properties = std::collections::HashMap::new();
    properties.insert("pc".to_string(), pc_hash);

    // Use enable_addr_auto() so the mDNS daemon fills in the real local
    // interface addresses at registration time.  Passing `()` alone leaves the
    // address set empty, which means the A record is never advertised and the
    // receiver's get_addresses() returns nothing — the root cause of LAN
    // connection failures on Windows.
    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &format!("cryptenv-share-{port}"),
        &hostname,
        (),
        port,
        Some(properties),
    )
    .map_err(|e| ShareError::Discovery(format!("build service info: {e}")))?
    .enable_addr_auto();

    mdns.register(service)
        .map_err(|e| ShareError::Discovery(format!("mdns register: {e}")))?;

    Ok((listener, port, mdns))
}

// ─── Connector ────────────────────────────────────────────────────────────────

/// Browse mDNS for `_cryptenv._tcp.local.` services, match the TXT `pc` field
/// against the hashed pairing code, and connect TCP to the matching service.
///
/// Returns a `TcpStream` on success or `ShareError::Timeout` if no matching
/// service is found within `timeout_secs`.
pub fn connect_to_peer(pairing_code: &str, timeout_secs: u64) -> Result<TcpStream, ShareError> {
    let expected_hash = hash_pairing_code(pairing_code);

    let mdns = ServiceDaemon::new()
        .map_err(|e| ShareError::Discovery(format!("mdns daemon: {e}")))?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .map_err(|e| ShareError::Discovery(format!("mdns browse: {e}")))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(ShareError::Timeout);
        }

        let event = match receiver.recv_timeout(remaining.min(Duration::from_millis(500))) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            // Check if this service's TXT pc field matches
            let pc_field = info.get_property_val_str("pc");
            if pc_field == Some(expected_hash.as_str()) {
                // Prefer IPv4 to avoid Windows link-local IPv6 routing issues.
                // Fall back to any available address only if no IPv4 is present.
                let addrs = info.get_addresses();
                let chosen = addrs
                    .iter()
                    .find(|a| a.is_ipv4())
                    .or_else(|| addrs.iter().next())
                    .cloned();

                let addr = match chosen {
                    Some(a) => a,
                    None => continue,
                };

                let socket_addr = std::net::SocketAddr::new(addr, info.get_port());

                let stream = TcpStream::connect_timeout(
                    &socket_addr,
                    Duration::from_secs(15),
                )
                .map_err(|e| ShareError::Io(format!("TCP connect to {socket_addr}: {e}")))?;

                drop(mdns); // unregisters the service when dropped
                return Ok(stream);
            }
        }
    }
}

// ─── Send session ─────────────────────────────────────────────────────────────


/// Perform the Hello handshake as sender.
/// Returns (stream, shared_key, fingerprint).
pub fn sender_handshake(
    mut stream: TcpStream,
    our_secret: &StaticSecret,
    our_pub: &PublicKey,
) -> Result<(TcpStream, Zeroizing<[u8; 32]>, String), ShareError> {
    // Send our public key first
    let our_pub_hex: String = our_pub
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    send_plain(
        &mut stream,
        &ShareMessage::Hello {
            version: 1,
            pubkey_hex: our_pub_hex,
        },
    )?;

    // Receive peer's Hello
    let peer_hello = recv_plain(&mut stream)?;
    let peer_pub_hex = match peer_hello {
        ShareMessage::Hello { pubkey_hex, .. } => pubkey_hex,
        _ => return Err(ShareError::Protocol("expected Hello message".into())),
    };

    let peer_pub_bytes = hex_decode(&peer_pub_hex)
        .map_err(|_| ShareError::Protocol("invalid peer pubkey hex".into()))?;
    if peer_pub_bytes.len() != 32 {
        return Err(ShareError::Protocol("peer pubkey must be 32 bytes".into()));
    }
    let peer_pub_arr: [u8; 32] = peer_pub_bytes.try_into().expect("length checked");
    let peer_pub = PublicKey::from(peer_pub_arr);

    let shared_key = derive_shared_key(our_secret, &peer_pub);
    // Sender is defined as having the lower pubkey lexicographically for determinism
    let fp = compute_fingerprint(our_pub.as_bytes(), &peer_pub_arr);

    Ok((stream, shared_key, fp))
}

/// Send items over an established, confirmed session.
/// `items_to_send` are already-decrypted PlainItem values.
pub fn sender_send_items(
    mut stream: TcpStream,
    shared_key: &Zeroizing<[u8; 32]>,
    items: Vec<PlainItem>,
) -> Result<usize, ShareError> {
    let _count = items.len();
    send_encrypted(
        &mut stream,
        &**shared_key,
        &ShareMessage::Items { items },
    )?;

    // Wait for Ack
    let ack = recv_encrypted(&mut stream, &**shared_key)?;
    match ack {
        ShareMessage::Ack { received } => Ok(received),
        ShareMessage::Error { message, .. } => Err(ShareError::Remote(message)),
        _ => Err(ShareError::Protocol("expected Ack message".into())),
    }
}

/// Send an Error message and close the connection.
pub fn sender_reject(
    mut stream: TcpStream,
    shared_key: &Zeroizing<[u8; 32]>,
    reason: &str,
) -> Result<(), ShareError> {
    send_encrypted(
        &mut stream,
        &**shared_key,
        &ShareMessage::Error {
            code: "REJECTED".into(),
            message: reason.to_string(),
        },
    )?;
    Ok(())
}

// ─── Receive session ──────────────────────────────────────────────────────────

/// Perform the Hello handshake as receiver.
/// Returns (stream, shared_key, fingerprint).
pub fn receiver_handshake(
    mut stream: TcpStream,
    our_secret: &StaticSecret,
    our_pub: &PublicKey,
) -> Result<(TcpStream, Zeroizing<[u8; 32]>, String), ShareError> {
    // Receive sender's Hello first
    let sender_hello = recv_plain(&mut stream)?;
    let sender_pub_hex = match sender_hello {
        ShareMessage::Hello { pubkey_hex, .. } => pubkey_hex,
        _ => return Err(ShareError::Protocol("expected Hello message".into())),
    };

    let sender_pub_bytes = hex_decode(&sender_pub_hex)
        .map_err(|_| ShareError::Protocol("invalid sender pubkey hex".into()))?;
    if sender_pub_bytes.len() != 32 {
        return Err(ShareError::Protocol("sender pubkey must be 32 bytes".into()));
    }
    let sender_pub_arr: [u8; 32] = sender_pub_bytes.try_into().expect("length checked");
    let sender_pub = PublicKey::from(sender_pub_arr);

    // Send our Hello
    let our_pub_hex: String = our_pub
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    send_plain(
        &mut stream,
        &ShareMessage::Hello {
            version: 1,
            pubkey_hex: our_pub_hex,
        },
    )?;

    let shared_key = derive_shared_key(our_secret, &sender_pub);
    // Fingerprint is computed with sender_pub first (same convention as sender side)
    let fp = compute_fingerprint(&sender_pub_arr, our_pub.as_bytes());

    Ok((stream, shared_key, fp))
}

/// Receive items from an established, confirmed session.
/// Returns the list of received PlainItems.
pub fn receiver_receive_items(
    mut stream: TcpStream,
    shared_key: &Zeroizing<[u8; 32]>,
) -> Result<Vec<PlainItem>, ShareError> {
    let msg = recv_encrypted(&mut stream, &**shared_key)?;
    match msg {
        ShareMessage::Items { items } => {
            let count = items.len();
            // Send acknowledgement
            send_encrypted(
                &mut stream,
                &**shared_key,
                &ShareMessage::Ack { received: count },
            )?;
            Ok(items)
        }
        ShareMessage::Error { message, .. } => Err(ShareError::Remote(message)),
        _ => Err(ShareError::Protocol("expected Items message".into())),
    }
}

// ─── Hex helper ───────────────────────────────────────────────────────────────

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}
