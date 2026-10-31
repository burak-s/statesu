use std::str::FromStr;
use std::sync::Arc;

use axum::Router;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::auth::repo::UserRepository;
use crate::auth::service::AuthService;
use crate::clock::Clock;
use crate::config::{Config, POOL_ACQUIRE_TIMEOUT};
use crate::crypto::email::EmailCipher;
use crate::crypto::jwt::JwtIssuer;
use crate::crypto::password::PasswordHasher;
use crate::error::{Error, InternalError};
use crate::http::rate_limit::RateLimiter;
use crate::http::static_files::StaticAssets;
use crate::http::{AppState, Limiters, build_router};
use crate::state::repo::StateRepository;
use crate::state::service::StateService;

/// The assembled application: SQLite pool, concrete services and the router.
/// Tests build exactly this composition root, so the binary and the test
/// harness exercise the same wiring.
pub struct App {
    pub state: AppState,
    pub router: Router,
    pool: SqlitePool,
}

impl App {
    /// Closes the SQLite pool so WAL checkpoints run on process exit.
    pub async fn close_database(&self) {
        self.pool.close().await;
    }
}

/// Docs CSP compatible with the Svelte build: inline Svelte/JSON-LD scripts,
/// Google Fonts stylesheets and font files, no other remote origins.
const DOCS_CSP: &str = "default-src 'self'; \
    script-src 'self' 'unsafe-inline'; \
    style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
    font-src 'self' https://fonts.gstatic.com; \
    connect-src 'self'; \
    img-src 'self' data:; \
    object-src 'none'; \
    base-uri 'self'; \
    frame-ancestors 'none'";

/// Builds the whole application from validated config, a clock and assets.
/// Opening the database, running migrations and wiring services all happen
/// here; `main` only adds env loading, runtime install and serving.
pub async fn build_app(
    config: Config,
    clock: Arc<dyn Clock>,
    assets: Arc<dyn StaticAssets>,
) -> Result<App, Error> {
    let pool = open_database(&config).await?;
    let users = UserRepository::new(pool.clone());
    let states = StateRepository::new(pool.clone());

    let cipher = EmailCipher::new(config.email_key, config.email_hmac_key.clone());
    let hasher = PasswordHasher::new(config.argon2_concurrency)?;
    let jwt = Arc::new(JwtIssuer::new(
        config.jwt_secret.clone(),
        config.jwt_audience.clone(),
        clock.clone(),
    ));

    let auth_service = AuthService::new(users.clone(), hasher, cipher.clone(), jwt.clone());
    let state_service = StateService::new(states, users.clone(), cipher.clone(), clock.clone());

    let limiters = Limiters {
        general: Arc::new(RateLimiter::per_minute(
            config.rate_limit_rpm,
            clock.clone(),
        )),
        auth: Arc::new(RateLimiter::per_minute(
            config.auth_rate_limit_rpm,
            clock.clone(),
        )),
    };

    let app_state = AppState {
        concurrency_gate: Arc::new(tokio::sync::Semaphore::new(config.max_concurrency)),
        config,
        auth_service,
        states: state_service,
        assets,
        limiters,
        csp: Arc::from(DOCS_CSP),
    };

    let router = build_router(app_state.clone());
    Ok(App {
        state: app_state,
        router,
        pool,
    })
}

async fn open_database(config: &Config) -> Result<SqlitePool, Error> {
    if let Some(parent) = std::path::Path::new(&config.db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Config(format!("database directory failed: {e}")))?;
    }

    let options = SqliteConnectOptions::from_str(&config.db_path)
        .map_err(|e| Error::Internal(InternalError::new(e)))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(POOL_ACQUIRE_TIMEOUT)
        .optimize_on_close(true, None);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .connect_with(options)
        .await
        .map_err(|e| Error::Config(format!("database connection failed: {e}")))?;

    // A legacy database schema must not be silently reused; the Rust schema
    // is UUIDv7/INTEGER-seconds and uses different table names.
    let legacy = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('user', 'state')",
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| Error::Internal(InternalError::new(e)))?;
    if legacy > 0 {
        return Err(Error::Config(
            "database contains a legacy schema; provision a fresh DB for the Rust backend"
                .to_owned(),
        ));
    }

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| Error::Internal(InternalError::new(e)))?;
    Ok(pool)
}
