use sqlx::SqlitePool;

use crate::error::{Error, InternalError};

/// Short-lived browser grants. Only code hashes persist; redemption is atomic.
#[derive(Clone)]
pub struct CliAuthorizationRepository {
    pool: SqlitePool,
}

impl CliAuthorizationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        code_hash: &[u8; 32],
        user_id: &uuid::Uuid,
        challenge: &str,
        redirect_uri: &str,
        now: i64,
    ) -> Result<(), Error> {
        sqlx::query("DELETE FROM cli_authorizations WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(InternalError::new(e)))?;
        sqlx::query("INSERT INTO cli_authorizations (code_hash, user_id, challenge, redirect_uri, expires_at) VALUES (?, ?, ?, ?, ?)")
            .bind(code_hash.as_slice())
            .bind(user_id.to_string())
            .bind(challenge)
            .bind(redirect_uri)
            .bind(now + 120)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(InternalError::new(e)))?;
        Ok(())
    }

    pub async fn consume(
        &self,
        code_hash: &[u8; 32],
        challenge: &str,
        redirect_uri: &str,
        now: i64,
    ) -> Result<uuid::Uuid, Error> {
        let user: Option<(String,)> = sqlx::query_as(
            "DELETE FROM cli_authorizations WHERE code_hash = ? AND challenge = ? AND redirect_uri = ? AND expires_at > ? RETURNING user_id",
        )
        .bind(code_hash.as_slice())
        .bind(challenge)
        .bind(redirect_uri)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(InternalError::new(e)))?;
        user.ok_or(Error::InvalidToken)?
            .0
            .parse()
            .map_err(|_| Error::InvalidToken)
    }
}
