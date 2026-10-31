use sqlx::sqlite::SqlitePool;

use crate::auth::UserRecord;
use crate::error::Error;

/// SQL-only access to the `users` table.
#[derive(Clone)]
pub struct UserRepository {
    pool: SqlitePool,
}

const COLUMNS: &str = "id, email_ciphertext, email_hash, password_hash, email_verified_at";

impl UserRepository {
    pub fn new(pool: SqlitePool) -> UserRepository {
        UserRepository { pool }
    }

    pub async fn create(
        &self,
        user_id: &uuid::Uuid,
        email_ciphertext: &[u8],
        email_hash: &[u8],
        password_hash: &str,
    ) -> Result<(), Error> {
        let query = "INSERT INTO users (id, email_ciphertext, email_hash, password_hash, email_verified_at) VALUES (?, ?, ?, ?, NULL)";
        let result = sqlx::query(query)
            .bind(user_id.to_string())
            .bind(email_ciphertext)
            .bind(email_hash)
            .bind(password_hash)
            // Suppress the generic sqlx error: a duplicate email_hash is the
            // domain-level "already exists" case, matching the pre-insert check.
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if e.as_database_error()
                    .is_some_and(|db| db.is_unique_violation())
                {
                    Error::UserAlreadyExists
                } else {
                    Error::Internal(crate::error::InternalError::new(e))
                }
            })?;
        if result.rows_affected() != 1 {
            return Err(Error::Internal(crate::error::InternalError::new(
                std::io::Error::other("user insert affected no row"),
            )));
        }
        Ok(())
    }

    pub async fn find_by_id(&self, user_id: &uuid::Uuid) -> Result<Option<UserRecord>, Error> {
        let row =
            sqlx::query_as::<_, UserRow>(&format!("SELECT {COLUMNS} FROM users WHERE id = ?"))
                .bind(user_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        row.map(UserRecord::try_from_row).transpose()
    }

    pub async fn find_by_email_hash(&self, email_hash: &[u8]) -> Result<Option<UserRecord>, Error> {
        let row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {COLUMNS} FROM users WHERE email_hash = ?"
        ))
        .bind(email_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        row.map(UserRecord::try_from_row).transpose()
    }

    /// Marks the address verified if it is not already; the SQL `COALESCE`
    /// keeps the first confirmation timestamp so replay cannot reset it.
    pub async fn mark_verified(&self, user_id: &uuid::Uuid, at_unix: i64) -> Result<(), Error> {
        sqlx::query(
            "UPDATE users SET email_verified_at = COALESCE(email_verified_at, ?) WHERE id = ?",
        )
        .bind(at_unix)
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }
}

fn internal(err: sqlx::Error) -> Error {
    Error::Internal(crate::error::InternalError::new(err))
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    email_ciphertext: Vec<u8>,
    email_hash: Vec<u8>,
    password_hash: String,
    email_verified_at: Option<i64>,
}

impl UserRecord {
    fn try_from_row(row: UserRow) -> Result<UserRecord, Error> {
        let id = row
            .id
            .parse::<uuid::Uuid>()
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(UserRecord {
            id,
            email_ciphertext: row.email_ciphertext,
            email_hash: row.email_hash,
            password_hash: row.password_hash,
            email_verified_at: row.email_verified_at,
        })
    }
}
