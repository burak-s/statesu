pub use rules::{MAX_PASSWORD_BYTES, MIN_PASSWORD_BYTES, check_password, valid_password};

use std::sync::Arc;
use std::time::Duration;

use argon2::{Argon2, Params, PasswordHash as PhcString, PasswordHasher as _, PasswordVerifier};
use tokio::sync::Semaphore;

use crate::error::{Error, InternalError};

mod rules {
    use crate::error::Error;

    pub const MIN_PASSWORD_BYTES: usize = 8;
    /// Above this length some legacy hashers truncate input, so the contract
    /// caps the UTF-8 byte count explicitly.
    pub const MAX_PASSWORD_BYTES: usize = 72;

    /// 8..=72 UTF-8 bytes with at least one Unicode letter and one numeric char.
    pub fn valid_password(password: &str) -> bool {
        let bytes = password.len();
        (MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&bytes)
            && password.chars().any(char::is_alphabetic)
            && password.chars().any(char::is_numeric)
    }

    pub fn check_password(password: &str) -> Result<(), Error> {
        if valid_password(password) {
            Ok(())
        } else {
            Err(Error::InvalidPassword)
        }
    }
}

/// Baseline Argon2id parameters (OWASP): 19 MiB memory, t=2, p=1.
const ARGON2_MEMORY_KIB: u32 = 19456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

/// Bound on how long a hash/verify call may wait for a worker slot before the
/// caller receives a controlled overload response.
const HASH_QUEUE_WAIT: Duration = Duration::from_secs(5);

/// Random fixed password for the dummy-verification path; it can never match a
/// registrant password because it is never used as one.
const DUMMY_PASSWORD: &str = "statesu unknown user probe";

/// `password_hash` errors only implement `Display`, so wrap them as strings.
fn hash_error<E: std::fmt::Display>(err: E) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(err.to_string())))
}

#[derive(Clone)]
pub struct PasswordHasher {
    argon2: Arc<Argon2<'static>>,
    permit_slots: Arc<Semaphore>,
    dummy_hash: Arc<String>,
}

impl PasswordHasher {
    pub fn new(argon2_concurrency: usize) -> Result<PasswordHasher, Error> {
        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            None,
        )
        .map_err(hash_error)?;
        let argon2 = Arc::new(Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        ));
        let salt = argon2::password_hash::SaltString::generate(
            &mut argon2::password_hash::rand_core::OsRng,
        );
        let dummy_hash = argon2
            .hash_password(DUMMY_PASSWORD.as_bytes(), &salt)
            .map_err(hash_error)?
            .to_string();
        Ok(PasswordHasher {
            argon2,
            permit_slots: Arc::new(Semaphore::new(argon2_concurrency.max(1))),
            dummy_hash: Arc::new(dummy_hash),
        })
    }

    pub async fn hash(&self, password: &str) -> Result<String, Error> {
        let argon2 = self.argon2.clone();
        let password = password.to_owned();
        self.run_bounded(move || {
            let salt = argon2::password_hash::SaltString::generate(
                &mut argon2::password_hash::rand_core::OsRng,
            );
            argon2
                .hash_password(password.as_bytes(), &salt)
                .map(|h| h.to_string())
                .map_err(hash_error)
        })
        .await
    }

    /// Verifies a password against a stored PHC string.
    pub async fn verify(&self, password: &str, phc_hash: &str) -> Result<bool, Error> {
        let argon2 = self.argon2.clone();
        let password = password.to_owned();
        let phc_hash = phc_hash.to_owned();
        self.run_bounded(move || {
            let parsed = PhcString::new(&phc_hash).map_err(hash_error)?;
            Ok(argon2.verify_password(password.as_bytes(), &parsed).is_ok())
        })
        .await
    }

    /// Login path for accounts that do not exist: one equally expensive verify
    /// against a hash generated once at startup.
    pub async fn verify_dummy(&self, password: &str) -> Result<(), Error> {
        self.verify(password, &self.dummy_hash).await.map(|_| ())
    }

    async fn run_bounded<F, R>(&self, work: F) -> Result<R, Error>
    where
        F: FnOnce() -> Result<R, Error> + Send + 'static,
        R: Send + 'static,
    {
        let permit =
            tokio::time::timeout(HASH_QUEUE_WAIT, self.permit_slots.clone().acquire_owned())
                .await
                .map_err(|_| Error::ServiceUnavailable)?
                .map_err(|_| Error::ServiceUnavailable)?;
        let result = tokio::task::spawn_blocking(work).await;
        drop(permit);
        result.map_err(|e| Error::Internal(InternalError::new(e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_and_verify_round_trip() {
        let hasher = PasswordHasher::new(1).unwrap();
        let hash = hasher.hash("correct horse 7").await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(hasher.verify("correct horse 7", &hash).await.unwrap());
        assert!(!hasher.verify("wrong password 7", &hash).await.unwrap());
    }

    #[tokio::test]
    async fn salts_are_fresh_per_hash() {
        let hasher = PasswordHasher::new(1).unwrap();
        let h1 = hasher.hash("значение 9").await.unwrap();
        let h2 = hasher.hash("значение 9").await.unwrap();
        assert_ne!(h1, h2);
    }

    #[tokio::test]
    async fn malformed_hash_is_internal_error() {
        let hasher = PasswordHasher::new(1).unwrap();
        let out = hasher.verify("whatever 5", "not-a-phc").await;
        assert!(matches!(out, Err(Error::Internal(InternalError { .. }))));
    }

    #[tokio::test]
    async fn dummy_verify_succeeds_without_matching() {
        let hasher = PasswordHasher::new(1).unwrap();
        hasher.verify_dummy("whatever 5").await.unwrap();
    }

    #[test]
    fn password_rule_table() {
        // Accept cases, including exact boundary lengths.
        assert!(valid_password("abcdefg1")); // 8 bytes
        assert!(valid_password(&format!("{}1", "a".repeat(71)))); // 72 bytes
        assert!(valid_password("şifre1lik")); // Unicode letters
        assert!(valid_password("١٢٣٤٥٦٧abc")); // Unicode digits
        // Reject cases.
        assert!(!valid_password("abcdef1")); // 7 bytes
        assert!(!valid_password(&format!("{}1", "a".repeat(72)))); // 73 bytes
        assert!(!valid_password("12345678"), "digits alone are rejected");
        assert!(!valid_password("abcdefghi"), "missing digit is rejected");
        assert!(!valid_password("a1"), "too short is rejected");
    }

    #[test]
    fn check_password_rejects_short_input() {
        assert!(matches!(
            check_password("short1"),
            Err(Error::InvalidPassword)
        ));
        assert!(check_password("abcdefg1").is_ok());
    }

    #[test]
    fn password_bytes_boundaries_are_exported() {
        assert_eq!(MIN_PASSWORD_BYTES, 8);
        assert_eq!(MAX_PASSWORD_BYTES, 72);
    }
}
