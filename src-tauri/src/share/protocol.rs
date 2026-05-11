use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;

use super::crypto::{decrypt_message, encrypt_message};
use super::package::PlainItem;
use super::ShareError;

/// All messages exchanged over the TCP share channel.
///
/// Pre-ECDH messages (Hello) are sent as plaintext JSON with a 4-byte LE length prefix.
/// Post-ECDH messages (Confirm, Items, Ack, Error) are AES-256-GCM encrypted, with the
/// same 4-byte LE prefix framing the encrypted payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ShareMessage {
    Hello {
        version: u8,
        pubkey_hex: String,
    },
    Confirm {
        accepted: bool,
    },
    Items {
        items: Vec<PlainItem>,
    },
    Ack {
        received: usize,
    },
    Error {
        code: String,
        message: String,
    },
}

// ─── Frame helpers ────────────────────────────────────────────────────────────

/// Write a 4-byte LE length prefix followed by `payload` bytes.
fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), ShareError> {
    if payload.len() > 16 * 1024 * 1024 {
        return Err(ShareError::Protocol("outgoing message too large".into()));
    }
    let len = payload.len() as u32;
    stream
        .write_all(&len.to_le_bytes())
        .map_err(|e| ShareError::Io(e.to_string()))?;
    stream
        .write_all(payload)
        .map_err(|e| ShareError::Io(e.to_string()))?;
    Ok(())
}

/// Read a 4-byte LE length prefix, then read exactly that many bytes.
/// Enforces a 16 MiB cap to prevent memory exhaustion on malformed input.
fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, ShareError> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| ShareError::Io(e.to_string()))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(ShareError::Protocol(format!(
            "incoming message length {len} exceeds 16 MiB limit"
        )));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .map_err(|e| ShareError::Io(e.to_string()))?;
    Ok(buf)
}

// ─── Public wire functions ────────────────────────────────────────────────────

/// Send a message as plaintext JSON (used before ECDH completes).
pub fn send_plain(stream: &mut TcpStream, msg: &ShareMessage) -> Result<(), ShareError> {
    let json =
        serde_json::to_vec(msg).map_err(|e| ShareError::Protocol(e.to_string()))?;
    write_frame(stream, &json)
}

/// Receive a plaintext JSON message (used before ECDH completes).
pub fn recv_plain(stream: &mut TcpStream) -> Result<ShareMessage, ShareError> {
    let frame = read_frame(stream)?;
    serde_json::from_slice(&frame).map_err(|e| ShareError::Protocol(e.to_string()))
}

/// Send a message encrypted with the session key.
pub fn send_encrypted(
    stream: &mut TcpStream,
    key: &[u8; 32],
    msg: &ShareMessage,
) -> Result<(), ShareError> {
    let json =
        serde_json::to_vec(msg).map_err(|e| ShareError::Protocol(e.to_string()))?;
    let ct = encrypt_message(key, &json);
    write_frame(stream, &ct)
}

/// Receive and decrypt a message with the session key.
pub fn recv_encrypted(
    stream: &mut TcpStream,
    key: &[u8; 32],
) -> Result<ShareMessage, ShareError> {
    let frame = read_frame(stream)?;
    let plaintext = decrypt_message(key, &frame)?;
    serde_json::from_slice(&plaintext).map_err(|e| ShareError::Protocol(e.to_string()))
}
