use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::ShareError;

/// Generate an ephemeral X25519 keypair.
pub fn generate_keypair() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random_from_rng(rand::thread_rng());
    let public = PublicKey::from(&secret);
    (secret, public)
}

/// Derive a 32-byte shared session key via X25519 + HKDF-SHA256.
/// info = b"cryptenv-share-v1" ensures domain separation.
pub fn derive_shared_key(
    secret: &StaticSecret,
    peer_pub: &PublicKey,
) -> Zeroizing<[u8; 32]> {
    let dh = secret.diffie_hellman(peer_pub);
    let hk = Hkdf::<Sha256>::new(None, dh.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"cryptenv-share-v1", &mut okm)
        .expect("HKDF expand with 32-byte output is always valid");
    Zeroizing::new(okm)
}

/// Compute fingerprint: first 8 hex chars of SHA-256(sender_pub || receiver_pub).
pub fn compute_fingerprint(sender_pub: &[u8; 32], receiver_pub: &[u8; 32]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(sender_pub);
    hasher.update(receiver_pub);
    let result = hasher.finalize();
    // First 4 bytes = 8 hex chars
    result[..4]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// AES-256-GCM encrypt. Prepends 12-byte random nonce to ciphertext.
pub fn encrypt_message(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("32-byte key is always valid");
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let mut ct = cipher
        .encrypt(nonce, plaintext)
        .expect("AES-GCM encrypt should not fail with valid inputs");
    let mut out = nonce_bytes.to_vec();
    out.append(&mut ct);
    out
}

/// AES-256-GCM decrypt. Expects nonce (12 bytes) prepended to ciphertext.
pub fn decrypt_message(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, ShareError> {
    if data.len() < 12 {
        return Err(ShareError::Protocol("message too short to contain nonce".into()));
    }
    let nonce = Nonce::from_slice(&data[..12]);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| ShareError::Crypto(e.to_string()))?;
    cipher
        .decrypt(nonce, &data[12..])
        .map_err(|_| ShareError::Crypto("decryption failed — message may be tampered".into()))
}

/// Derive a package encryption key from a passphrase using Argon2id.
/// Uses lighter parameters than the vault KDF because the passphrase is a
/// random 12-char string (high entropy), so lower work factors are acceptable.
pub fn derive_package_key(passphrase: &str, salt: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let params = Params::new(32768, 2, 2, Some(32)).expect("valid Argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .expect("Argon2id key derivation failed");
    Zeroizing::new(key)
}

/// Generate a 12-character random passphrase from [a-zA-Z0-9].
pub fn generate_passphrase() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let mut out = String::with_capacity(12);
    let mut buf = [0u8; 12];
    rng.fill_bytes(&mut buf);
    for byte in buf {
        out.push(ALPHABET[(byte as usize) % ALPHABET.len()] as char);
    }
    out
}
