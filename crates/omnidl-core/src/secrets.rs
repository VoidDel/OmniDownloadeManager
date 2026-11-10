//! Encryption of what a task stores about its requests (docs/development-plan.md §4.6): cookies
//! and extra headers come from the browser and can be session credentials. A task whose options
//! carry any is stored as `enc:v1:` + base64(nonce ‖ AES-256-GCM ciphertext); the app keeps the
//! key (see its `secrets` module).

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD};

/// Marks sealed values in the database.
pub const SEALED_PREFIX: &str = "enc:v1:";

/// Seals and opens stored values.
pub trait Secrets: Send + Sync {
    /// The sealed form of `plain`, or `None` to store it as it is.
    fn seal(&self, plain: &[u8]) -> Option<String>;
    /// The plain bytes of a value from [`seal`](Self::seal); `None` when it cannot be opened.
    fn open(&self, sealed: &str) -> Option<Vec<u8>>;
}

/// Stores everything as it is (tests, or no key available).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSecrets;

impl Secrets for NoSecrets {
    fn seal(&self, _: &[u8]) -> Option<String> {
        None
    }

    fn open(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

/// AES-256-GCM with a random nonce per value.
pub struct AesSecrets {
    cipher: Aes256Gcm,
}

impl AesSecrets {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(key.into()),
        }
    }

    /// A new random key (from the operating system's generator).
    pub fn generate_key() -> [u8; 32] {
        Aes256Gcm::generate_key(OsRng).into()
    }
}

impl Secrets for AesSecrets {
    fn seal(&self, plain: &[u8]) -> Option<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self.cipher.encrypt(&nonce, plain).ok()?;
        let mut out = nonce.to_vec();
        out.extend(ct);
        Some(format!("{SEALED_PREFIX}{}", STANDARD.encode(out)))
    }

    fn open(&self, sealed: &str) -> Option<Vec<u8>> {
        let raw = STANDARD.decode(sealed.strip_prefix(SEALED_PREFIX)?).ok()?;
        if raw.len() < 12 {
            return None;
        }
        let (nonce, ct) = raw.split_at(12);
        self.cipher.decrypt(Nonce::from_slice(nonce), ct).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let s = AesSecrets::new(&[7; 32]);
        let sealed = s.seal(b"session=abc").unwrap();
        assert!(sealed.starts_with(SEALED_PREFIX));
        // The stored bytes do not carry the plain text. (Not checked on the base64 text: a random
        // nonce and ciphertext spell "abc" there now and then.)
        let raw = STANDARD.decode(&sealed[SEALED_PREFIX.len()..]).unwrap();
        assert!(!raw.windows(11).any(|w| w == b"session=abc"));
        assert_eq!(s.open(&sealed).unwrap(), b"session=abc");
        assert_ne!(s.seal(b"session=abc").unwrap(), sealed, "fresh nonce");
        // Another key, a tampered value, or plain text do not open.
        assert!(AesSecrets::new(&[8; 32]).open(&sealed).is_none());
        let mut bad = sealed.clone();
        bad.pop();
        bad.push('A');
        assert!(s.open(&bad).is_none() || bad == sealed);
        assert!(s.open("{}").is_none());
    }
}
