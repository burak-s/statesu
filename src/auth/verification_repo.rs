use sqlx::sqlite::SqlitePool;

use crate::error::Error;

/// SQL-only access to `email_verification_tokens`: single-use, expiring
/// tokens keyed by hash; issuing a new one replaces every earlier token of
/// the account, so at most one outstanding link exists at a time.
#[derive(Clone)]
pub struct VerificationTokenRepository {
    pool: SqlitePool,
}

/// Verification links live for half an hour — short enough to lose nothing,
/// long enough for SPAM-folder detours.
pub const VERIFICATION_TTL_SECS: i64 = 30 * 60;

/// Minimum seconds between two issued verification mails per account.
pub const RESEND_COOLDOWN_SECS: i64 = 60;

pub struct VerificationTokenRow {
    pub user_id: uuid::Uuid,
    pub created_at: i64,
    pub expires_at: i64,
    pub consumed_at: Option<i64>,
}

impl VerificationTokenRepository {
    pub fn new(pool: SqlitePool) -> VerificationTokenRepository {
        VerificationTokenRepository { pool }
    }

    /// Stores a new token, invalidating all earlier ones of the account.
    pub async fn issue(
        &self,
        token_hash: &[u8; 32],
        user_id: &uuid::Uuid,
        now_unix: i64,
    ) -> Result<(), Error> {
        let expires_at = now_unix.saturating_add(VERIFICATION_TTL_SECS);
        sqlx::query("DELETE FROM email_verification_tokens WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        sqlx::query(
            "INSERT INTO email_verification_tokens (token_hash, user_id, created_at, expires_at, consumed_at) \
             VALUES (?, ?, ?, ?, NULL)",
        )
        .bind(token_hash.as_slice())
        .bind(user_id.to_string())
        .bind(now_unix)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(())
    }

    /// The newest outstanding token of an account, if any; used for
    /// resend cooldown checks.
    pub async fn latest_for_user(
        &self,
        user_id: &uuid::Uuid,
    ) -> Result<Option<VerificationTokenRow>, Error> {
        sqlx::query_as::<_, VerificationTokenSqlRow>(
            "SELECT user_id, created_at, expires_at, consumed_at \
             FROM email_verification_tokens WHERE user_id = ? \
             ORDER BY created_at DESC, token_hash LIMIT 1",
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))
    }

    /// Loads one token by hash for the consume decision.
    pub async fn find(&self, token_hash: &[u8; 32]) -> Result<Option<VerificationTokenRow>, Error> {
        sqlx::query_as::<_, VerificationTokenSqlRow>(
            "SELECT user_id, created_at, expires_at, consumed_at \
             FROM email_verification_tokens WHERE token_hash = ?",
        )
        .bind(token_hash.as_slice())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))
    }

    /// Atomically consumes an unexpired token; `false` when another request
    /// got there first. Single-use races resolve to exactly one winner.
    pub async fn consume(&self, token_hash: &[u8; 32], now_unix: i64) -> Result<bool, Error> {
        let result = sqlx::query(
            "UPDATE email_verification_tokens SET consumed_at = ? \
             WHERE token_hash = ? AND consumed_at IS NULL",
        )
        .bind(now_unix)
        .bind(token_hash.as_slice())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(result.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct VerificationTokenSqlRow {
    user_id: String,
    created_at: i64,
    expires_at: i64,
    consumed_at: Option<i64>,
}

impl From<VerificationTokenSqlRow> for VerificationTokenRow {
    fn from(row: VerificationTokenSqlRow) -> VerificationTokenRow {
        VerificationTokenRow {
            user_id: uuid::Uuid::parse_str(&row.user_id)
                .expect("schema stores canonical UUID text"),
            created_at: row.created_at,
            expires_at: row.expires_at,
            consumed_at: row.consumed_at,
        }
    }
}
