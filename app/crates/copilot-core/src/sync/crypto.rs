//! Sync encryption: boring, standard constructions only.
//!
//! - Key: Argon2id(passphrase, salt) → 32 bytes. The salt lives beside the
//!   remote data (`key.salt`, public by design); the passphrase never leaves
//!   the machine. Losing it is unrecoverable — no backdoor, documented.
//! - Blobs: XChaCha20-Poly1305 AEAD, fresh random 24-byte nonce per object,
//!   stored as `nonce || ciphertext`.
//! - Names: HMAC-SHA256(key, plaintext-name) so the remote learns sizes and
//!   access patterns only — never filenames or content hashes.

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("wrong passphrase or corrupted data")]
    Decrypt,
    #[error("key derivation failed: {0}")]
    Kdf(String),
}

/// A derived library key. Zeroized-on-drop niceties are deliberately skipped
/// in favor of simplicity; the key also lives in the OS keychain.
#[derive(Clone)]
pub struct LibraryKey(pub [u8; KEY_LEN]);

impl std::fmt::Debug for LibraryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LibraryKey(…)") // never print key material
    }
}

pub fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    use chacha20poly1305::aead::rand_core::RngCore;
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Argon2id with the crate's recommended defaults (m=19 MiB, t=2, p=1).
pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<LibraryKey, CryptoError> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    Ok(LibraryKey(key))
}

pub fn encrypt(key: &LibraryKey, plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("XChaCha20-Poly1305 encryption is infallible for in-memory data");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn decrypt(key: &LibraryKey, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if blob.len() < NONCE_LEN {
        return Err(CryptoError::Decrypt);
    }
    let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

/// Opaque remote name for a logical object: HMAC keyed by the library key,
/// so the remote can't correlate names with public content hashes.
pub fn blob_name(key: &LibraryKey, logical_name: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key.0).expect("any key length");
    mac.update(logical_name.as_bytes());
    let digest = mac.finalize().into_bytes();
    digest.iter().take(20).map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_passphrase_fails_cleanly() {
        let salt = random_salt();
        let key = derive_key("correct horse battery staple", &salt).unwrap();
        let blob = encrypt(&key, b"learner memory stays sealed");
        assert_ne!(&blob[24..], b"learner memory stays sealed".as_slice());
        assert_eq!(
            decrypt(&key, &blob).unwrap(),
            b"learner memory stays sealed"
        );

        let wrong = derive_key("correct horse battery stapl", &salt).unwrap();
        assert!(matches!(decrypt(&wrong, &blob), Err(CryptoError::Decrypt)));

        // Same passphrase, different salt → different key (per-library).
        let other_salt = random_salt();
        let other = derive_key("correct horse battery staple", &other_salt).unwrap();
        assert!(matches!(decrypt(&other, &blob), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn nonces_are_fresh_and_names_are_opaque_but_stable() {
        let key = derive_key("p", &random_salt()).unwrap();
        let a = encrypt(&key, b"same plaintext");
        let b = encrypt(&key, b"same plaintext");
        assert_ne!(a, b, "fresh nonce per encryption");
        assert_eq!(decrypt(&key, &a).unwrap(), decrypt(&key, &b).unwrap());

        let name1 = blob_name(&key, "notes/notes.jsonl");
        let name2 = blob_name(&key, "notes/notes.jsonl");
        assert_eq!(name1, name2, "deterministic per key");
        assert_eq!(name1.len(), 40);
        assert!(!name1.contains("notes"), "opaque");
        let other_key = derive_key("q", &random_salt()).unwrap();
        assert_ne!(name1, blob_name(&other_key, "notes/notes.jsonl"));
    }

    #[test]
    fn corrupted_ciphertext_is_rejected() {
        let key = derive_key("p", &random_salt()).unwrap();
        let mut blob = encrypt(&key, b"integrity matters");
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(matches!(decrypt(&key, &blob), Err(CryptoError::Decrypt)));
    }
}
