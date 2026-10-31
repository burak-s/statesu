use sqlx::sqlite::SqlitePool;

use crate::error::{Error, InternalError};
use crate::state::{RawStateRow, StateRow};

/// SQL-only access to the `states` table. Orderings always include an `id`
/// tiebreak, so results stay deterministic at second-granularity timestamps.
#[derive(Clone)]
pub struct StateRepository {
    pool: SqlitePool,
}

impl StateRepository {
    pub fn new(pool: SqlitePool) -> StateRepository {
        StateRepository { pool }
    }

    pub async fn insert(
        &self,
        state_id: &uuid::Uuid,
        user_id: &uuid::Uuid,
        text: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<(), Error> {
        let query =
            "INSERT INTO states (id, user_id, text, created_at, expires_at) VALUES (?, ?, ?, ?, ?)";
        let result = sqlx::query(query)
            .bind(state_id.to_string())
            .bind(user_id.to_string())
            .bind(text)
            .bind(created_at)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(|e| error_sql(e, "state insert"))?;
        if result.rows_affected() != 1 {
            return Err(Error::Internal(InternalError::new(std::io::Error::other(
                "state insert affected no row",
            ))));
        }
        Ok(())
    }

    /// One page for one owner, newest first (created_at DESC, id DESC).
    pub async fn page_for_user(
        &self,
        user_id: &uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<StateRow>, Error> {
        let rows = sqlx::query_as::<_, RawStateRow>(
            "SELECT id, user_id, text, created_at, expires_at \
             FROM states WHERE user_id = ? \
             ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(user_id.to_string())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| error_sql(e, "state page query"))?;
        rows.into_iter().map(RawStateRow::into_state_row).collect()
    }

    pub async fn count_for_user(&self, user_id: &uuid::Uuid) -> Result<i64, Error> {
        let row = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM states WHERE user_id = ?")
            .bind(user_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| error_sql(e, "state count"))?;
        Ok(row)
    }

    pub async fn latest_for_user(&self, user_id: &uuid::Uuid) -> Result<Option<StateRow>, Error> {
        let row = sqlx::query_as::<_, RawStateRow>(
            "SELECT id, user_id, text, created_at, expires_at \
             FROM states WHERE user_id = ? \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| error_sql(e, "state latest query"))?;
        row.map(RawStateRow::into_state_row).transpose()
    }

    pub async fn latest_global(&self) -> Result<Option<StateRow>, Error> {
        let row = sqlx::query_as::<_, RawStateRow>(
            "SELECT id, user_id, text, created_at, expires_at \
             FROM states \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| error_sql(e, "global latest query"))?;
        row.map(RawStateRow::into_state_row).transpose()
    }

    /// Ownership is part of the SQL predicate; zero affected rows means the
    /// state is missing or foreign, both mapped to the same 404.
    pub async fn delete(&self, state_id: &uuid::Uuid, user_id: &uuid::Uuid) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM states WHERE id = ? AND user_id = ?")
            .bind(state_id.to_string())
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| error_sql(e, "state delete"))?;
        Ok(result.rows_affected() == 1)
    }
}

fn error_sql(err: sqlx::Error, context: &'static str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(format!(
        "{context}: {err}"
    ))))
}
