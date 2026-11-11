use std::sync::Arc;

use aes_gcm::Key;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::{Error, InternalError};

/// Storage layout for AES-256-GCM sealed secrets: byte 0 is a format
/// version, bytes 1..13 a fresh nonce, the remainder ciphertext with tag.
/// Version 1 binds the ciphertext to one owner UUID via AAD, so a blob from
/// one storage connection can never be decrypted as another.
const SECRET_CIPHER_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const VERSION_LEN: usize = 1;

/// Seals server-sensitive secrets (user-supplied R2 access keys) with a
/// dedicated key. Same format discipline as the email cipher; a separate
/// type keeps the two key domains from ever being swapped by accident.
#[derive(Clone)]
pub struct SecretCipher {
    aead: Arc<Aes256Gcm>,
}

impl SecretCipher {
    pub fn new(key: [u8; 32]) -> SecretCipher {
        SecretCipher {
            aead: Arc::new(Aes256Gcm::new(&Key::<Aes256Gcm>::from(key))),
        }
    }

    pub fn seal(&self, secret: &[u8], context: &uuid::Uuid) -> Result<Vec<u8>, Error> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce_bytes).map_err(internal)?;
        let payload = Payload {
            msg: secret,
            aad: context.as_bytes(),
        };
        let ciphertext = self
            .aead
            .encrypt(Nonce::from_slice(&nonce_bytes), payload)
            .map_err(internal)?;
        let mut blob = Vec::with_capacity(VERSION_LEN + NONCE_LEN + ciphertext.len());
        blob.push(SECRET_CIPHER_VERSION);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    pub fn open(&self, blob: &[u8], context: &uuid::Uuid) -> Result<Vec<u8>, Error> {
        let (_, rest) = blob
            .split_first_chunk::<1>()
            .filter(|(version, _)| version[0] == SECRET_CIPHER_VERSION)
            .ok_or_else(sealed_corrupt)?;
        let (nonce_bytes, ciphertext) = rest
            .split_first_chunk::<NONCE_LEN>()
            .ok_or_else(sealed_corrupt)?;
        let payload = Payload {
            msg: ciphertext,
            aad: context.as_bytes(),
        };
        self.aead
            .decrypt(Nonce::from_slice(nonce_bytes), payload)
            .map_err(|_| sealed_corrupt())
    }
}

fn internal<E: std::fmt::Display>(err: E) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(err.to_string())))
}

fn sealed_corrupt() -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(
        "malformed sealed secret",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> SecretCipher {
        SecretCipher::new([4u8; 32])
    }

    #[test]
    fn roundtrip_is_context_bound() {
        let c = cipher();
        let a = uuid::Uuid::now_v7();
        let b = uuid::Uuid::now_v7();
        let blob = c.seal(b"top-secret-key", &a).unwrap();
        assert_eq!(blob[0], SECRET_CIPHER_VERSION);
        assert_eq!(c.open(&blob, &a).unwrap(), b"top-secret-key");
        assert!(c.open(&blob, &b).is_err());
    }

    #[test]
    fn fresh_nonce_per_seal() {
        let c = cipher();
        let a = uuid::Uuid::now_v7();
        assert_ne!(c.seal(b"k", &a).unwrap(), c.seal(b"k", &a).unwrap());
    }

    #[test]
    fn corrupted_or_foreign_version_rejected() {
        let c = cipher();
        let a = uuid::Uuid::now_v7();
        assert!(c.open(&[1, 2, 3], &a).is_err());
        assert!(c.open(&[99, 0, 0, 0], &a).is_err());
    }
}
