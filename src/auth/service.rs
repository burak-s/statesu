use std::sync::Arc;

use crate::auth::repo::UserRepository;
use crate::auth::{AuthResponse, CredentialsRequest, MeResponse, UserRecord};
use crate::crypto::email::EmailCipher;
use crate::crypto::email::{normalize_email, validate_email};
use crate::crypto::jwt::JwtIssuer;
use crate::crypto::password::PasswordHasher;
use crate::error::Error;

/// Account rules: uniqueness, hashing, credential checks and token issuance.
/// Repos and crypto components are injected; no globals.
#[derive(Clone)]
pub struct AuthService {
    users: UserRepository,
    hasher: PasswordHasher,
    cipher: EmailCipher,
    jwt: Arc<JwtIssuer>,
}

impl AuthService {
    pub fn new(
        users: UserRepository,
        hasher: PasswordHasher,
        cipher: EmailCipher,
        jwt: Arc<JwtIssuer>,
    ) -> AuthService {
        AuthService {
            users,
            hasher,
            cipher,
            jwt,
        }
    }

    pub async fn register(&self, request: &CredentialsRequest) -> Result<AuthResponse, Error> {
        let email = normalize_email(&request.email);
        validate_email(&email)?;
        crate::crypto::password::check_password(&request.password)?;

        let user_id = uuid::Uuid::now_v7();
        let ciphertext = self.cipher.encrypt(&email, &user_id)?;
        let email_hash = self.cipher.email_hash(&email);

        // Hash before uniqueness checks so existing accounts and new ones cost
        // the same, keeping register timing membership-neutral.
        let password_hash = self.hasher.hash(&request.password).await?;
        self.users
            .create(&user_id, &ciphertext, &email_hash, &password_hash)
            .await?;
        self.issue_token_for(&user_id, &email)
    }

    /// Generic failure mode: unknown accounts run a dummy hash verify and
    /// return the same error as wrong passwords; no user enumeration.
    pub async fn login(&self, request: &CredentialsRequest) -> Result<AuthResponse, Error> {
        let email = normalize_email(&request.email);
        if email.is_empty() || request.password.is_empty() {
            return Err(Error::InvalidCredentials);
        }
        let email_hash = self.cipher.email_hash(&email);
        let user = match self.users.find_by_email_hash(&email_hash).await? {
            Some(user) => user,
            None => {
                self.hasher.verify_dummy(&request.password).await?;
                return Err(Error::InvalidCredentials);
            }
        };
        if !self
            .hasher
            .verify(&request.password, &user.password_hash)
            .await?
        {
            return Err(Error::InvalidCredentials);
        }
        let stored_email = self.cipher.decrypt(&user.email_ciphertext, &user.id)?;
        self.issue_token_for(&user.id, &stored_email)
    }

    pub async fn me(&self, user: &UserRecord) -> Result<MeResponse, Error> {
        let email = self.cipher.decrypt(&user.email_ciphertext, &user.id)?;
        Ok(MeResponse { id: user.id, email })
    }

    /// Bearer-token authentication: strict JWT validation plus an account
    /// existence check, so deleted accounts fail closed as `invalid token`.
    pub async fn authenticate(&self, token: &str) -> Result<UserRecord, Error> {
        let claims = self.jwt.verify(token)?;
        let user_id = claims
            .sub
            .parse::<uuid::Uuid>()
            .map_err(|_| Error::InvalidToken)?;
        self.users
            .find_by_id(&user_id)
            .await?
            .ok_or(Error::InvalidToken)
    }

    fn issue_token_for(&self, user_id: &uuid::Uuid, email: &str) -> Result<AuthResponse, Error> {
        let (token, expires_at) = self.jwt.issue(user_id)?;
        Ok(AuthResponse {
            id: *user_id,
            email: email.to_owned(),
            token,
            expires_at,
        })
    }
}
