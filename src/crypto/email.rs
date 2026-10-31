use std::sync::Arc;

use aes_gcm::Key;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::Mac;

use crate::error::{Error, InternalError};

/// Storage layout: byte 0 is a format version, bytes 1..13 the fresh nonce,
/// the remainder the AES-256-GCM ciphertext with tag. Version 1 binds the
/// ciphertext to the owning user's UUID bytes via AAD.
const EMAIL_CIPHER_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const VERSION_LEN: usize = 1;

/// Maximum byte length of an email address after normalization.
pub const MAX_EMAIL_LEN: usize = 254;

#[derive(Clone)]
pub struct EmailCipher {
    aead: Arc<Aes256Gcm>,
    hmac_key: Arc<Vec<u8>>,
}

impl EmailCipher {
    pub fn new(email_key: [u8; 32], email_hmac_key: Vec<u8>) -> EmailCipher {
        EmailCipher {
            aead: Arc::new(Aes256Gcm::new(&Key::<Aes256Gcm>::from(email_key))),
            hmac_key: Arc::new(email_hmac_key),
        }
    }

    /// Encrypts `email` (already normalized by the caller). The AAD binds the
    /// ciphertext to `user_id` so rows cannot be re-parented between users.
    pub fn encrypt(&self, email: &str, user_id: &uuid::Uuid) -> Result<Vec<u8>, Error> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce_bytes).map_err(|e| {
            Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
        })?;
        let payload = Payload {
            msg: email.as_bytes(),
            aad: user_id.as_bytes(),
        };
        let ciphertext = self
            .aead
            .encrypt(Nonce::from_slice(&nonce_bytes), payload)
            .map_err(|e| {
                Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
            })?;
        let mut blob = Vec::with_capacity(VERSION_LEN + NONCE_LEN + ciphertext.len());
        blob.push(EMAIL_CIPHER_VERSION);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    pub fn decrypt(&self, blob: &[u8], user_id: &uuid::Uuid) -> Result<String, Error> {
        let (_, rest) = blob
            .split_first_chunk::<1>()
            .filter(|(v, _)| v[0] == EMAIL_CIPHER_VERSION)
            .ok_or_else(invalid_ciphertext)?;
        let (nonce_bytes, ciphertext) = rest
            .split_first_chunk::<NONCE_LEN>()
            .ok_or_else(invalid_ciphertext)?;
        let payload = Payload {
            msg: ciphertext,
            aad: user_id.as_bytes(),
        };
        let plaintext = self
            .aead
            .decrypt(Nonce::from_slice(nonce_bytes), payload)
            .map_err(|e| {
                Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
            })?;
        String::from_utf8(plaintext)
            .map_err(|e| Error::Internal(InternalError::new(std::io::Error::other(e.to_string()))))
    }

    /// HMAC-SHA256 over the normalized email: the key-cloaked lookup key that
    /// reaches SQL. Normalization lives here so every lookup path is
    /// case-insensitive and whitespace-insensitive by construction.
    pub fn email_hash(&self, email: &str) -> Vec<u8> {
        let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(&self.hmac_key)
            .expect("HMAC accepts any key length");
        mac.update(normalize_email(email).as_bytes());
        mac.finalize().into_bytes().as_slice().to_vec()
    }
}

fn invalid_ciphertext() -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(
        "malformed email ciphertext",
    )))
}

/// Trims surrounding Unicode whitespace and lowercases. The canonical form for
/// storage, lookup and display.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn validate_email(normalized: &str) -> Result<(), Error> {
    let valid = !normalized.is_empty()
        && normalized.len() <= MAX_EMAIL_LEN
        && normalized.is_ascii()
        && normalized.matches('@').count() == 1
        && normalized
            .split_once('@')
            .map(|(local, domain)| validate_local(local) && validate_domain(domain))
            .unwrap_or(false);
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidEmail)
    }
}

fn validate_local(local: &str) -> bool {
    if local.is_empty() || local.len() > 64 {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    local.chars().all(is_local_char)
}

fn validate_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    if domain.starts_with('.') || domain.ends_with('.') {
        return false;
    }
    domain.split('.').all(validate_domain_label)
}

fn validate_domain_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }
    !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_local_char(c: char) -> bool {
    // Restrictive, dot-atom-style local part: no quoted strings, ASCII only.
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '/'
                | '='
                | '?'
                | '^'
                | '_'
                | '`'
                | '{'
                | '|'
                | '}'
                | '~'
                | '.'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn cipher() -> (EmailCipher, EmailCipher) {
        (
            EmailCipher::new([5u8; 32], vec![9u8; 32]),
            EmailCipher::new([5u8; 32], vec![8u8; 32]),
        )
    }

    #[test]
    fn normalization_trims_and_lowers() {
        assert_eq!(
            normalize_email("  \t Visitor@EXAMPLE.com\n"),
            "visitor@example.com"
        );
        assert_eq!(normalize_email("USER+tag@Mail.Org"), "user+tag@mail.org");
    }

    #[test]
    fn email_validation_accept_table() {
        let local_max = format!("{}@example.com", "a".repeat(64));
        for e in [
            "a@b.co".to_owned(),
            "user.name+tag@sub.domain.example".to_owned(),
            "o'brien@example.net".to_owned(),
            "user!#$%&'*+-/=?^_`{|}~@example.org".to_owned(),
            local_max,
        ] {
            assert!(validate_email(&e).is_ok(), "{e} should be valid");
        }
    }

    #[test]
    fn email_validation_reject_table() {
        for e in [
            String::new(),
            "no-at-sign".to_owned(),
            "two@@example.com".to_owned(),
            "user@".to_owned(),
            "@example.com".to_owned(),
            ".user@example.com".to_owned(),
            "user.@example.com".to_owned(),
            "user..name@example.com".to_owned(),
            "user@-bad.com".to_owned(),
            "user@bad-.com".to_owned(),
            "user@b..com".to_owned(),
            "user@.example.com".to_owned(),
            "user@example.com.".to_owned(),
            format!("{}@example.com", "l".repeat(65)),
            format!("a@{}", "d".repeat(254)),
            "münchen@example.com".to_owned(),
            "user@exämple.com".to_owned(),
            "user@example.invalid familiares".to_owned(),
        ] {
            assert!(validate_email(&e).is_err(), "{e} should be invalid");
        }
    }

    #[test]
    fn aes_gcm_roundtrip_and_aad_binding() {
        let (c, _) = cipher();
        let a = uuid::Uuid::now_v7();
        let b = uuid::Uuid::now_v7();
        let blob = c.encrypt("user@example.com", &a).unwrap();
        assert_eq!(blob[0], EMAIL_CIPHER_VERSION);
        assert_eq!(c.decrypt(&blob, &a).unwrap(), "user@example.com");
        assert!(c.decrypt(&blob, &b).is_err());
    }

    #[test]
    fn fresh_gcm_nonce_per_encryption() {
        let (c, _) = cipher();
        let a = uuid::Uuid::now_v7();
        let e1 = c.encrypt("same@example.com", &a).unwrap();
        let e2 = c.encrypt("same@example.com", &a).unwrap();
        assert_ne!(e1, e2);
    }

    #[test]
    fn malformed_ciphertext_rejected() {
        let (c, _) = cipher();
        let a = uuid::Uuid::now_v7();
        let short = [EMAIL_CIPHER_VERSION, 1, 2];
        assert!(matches!(
            c.decrypt(&short, &a),
            Err(Error::Internal(InternalError { .. }))
        ));
        let bad_version = vec![EMAIL_CIPHER_VERSION + 1, 0, 0, 0];
        assert!(c.decrypt(&bad_version, &a).is_err());
    }

    #[test]
    fn email_hash_deterministic_and_key_bound() {
        let (c1, c2) = cipher();
        let h1 = c1.email_hash("user@example.com");
        assert_eq!(h1, c1.email_hash("user@example.com"));
        assert_eq!(h1, c1.email_hash("  USER@Example.COM "));
        assert_eq!(h1.len(), 32);
        assert_ne!(h1, c2.email_hash("user@example.com"));
    }

    #[test]
    fn max_email_len_constant_matches_contract() {
        assert_eq!(MAX_EMAIL_LEN, 254);
    }
}
