use std::str::FromStr;
use std::sync::Arc;

use axum::Router;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::auth::pat_repo::PatRepository;
use crate::auth::repo::UserRepository;
use crate::auth::service::AuthService;
use crate::auth::session_repo::SessionRepository;
use crate::auth::turnstile::{CloudflareTurnstile, TurnstileVerifier};
use crate::auth::verification_repo::VerificationTokenRepository;
use crate::blog::repo::BlogRepository;
use crate::blog::service::BlogService;
use crate::clock::Clock;
use crate::config::{Config, POOL_ACQUIRE_TIMEOUT};
use crate::crypto::email::EmailCipher;
use crate::crypto::password::PasswordHasher;
use crate::error::{Error, InternalError};
use crate::http::rate_limit::RateLimiter;
use crate::http::static_files::StaticAssets;
use crate::http::{AppState, Limiters, build_router};
use crate::post::repo::PostRepository;
use crate::post::service::PostService;

/// The assembled application: SQLite pool, concrete services and the router.
/// Tests build exactly this composition root, so the binary and the test
/// harness exercise the same wiring.
pub struct App {
    pub state: AppState,
    pub router: Router,
    /// Exposed so the test harness can simulate states writes never reach,
    /// such as a quota lowered below existing usage.
    pub pool: SqlitePool,
}

impl App {
    /// Closes the SQLite pool so WAL checkpoints run on process exit.
    pub async fn close_database(&self) {
        self.pool.close().await;
    }
}

/// Docs CSP compatible with the Svelte build: inline Svelte/JSON-LD scripts,
/// Google Fonts stylesheets and font files, no other remote origins.
// The Turnstile origins cover the login/register widget: its script comes
// from `challenges.cloudflare.com` and renders inside an iframe from the
// same origin. The dashboard pages are served with these headers too.
const DOCS_CSP: &str = "default-src 'self'; \
    script-src 'self' 'unsafe-inline' https://challenges.cloudflare.com; \
    style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
    font-src 'self' https://fonts.gstatic.com; \
    connect-src 'self'; \
    frame-src https://challenges.cloudflare.com; \
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
    let mailer: Arc<dyn crate::mail::Mailer> = Arc::new(crate::mail::ResendMailer::new(
        config.resend_api_key.clone(),
        config.mail_from.clone(),
    )?);
    let turnstile: Option<Arc<dyn TurnstileVerifier>> = match config.turnstile_secret.clone() {
        Some(secret) => Some(Arc::new(CloudflareTurnstile::new(secret)?)),
        None => None,
    };
    build_app_with_turnstile(config, clock, assets, mailer, turnstile).await
}

/// Composition root with an injected mailer; tests substitute a recorder
/// here so nothing leaves the machine.
pub async fn build_app_with_mailer(
    config: Config,
    clock: Arc<dyn Clock>,
    assets: Arc<dyn StaticAssets>,
    mailer: Arc<dyn crate::mail::Mailer>,
) -> Result<App, Error> {
    build_app_with_turnstile(config, clock, assets, mailer, None).await
}

/// Composition root additionally accepting a human-verification seam;
/// `None` means Turnstile is disabled even with a configured secret.
pub async fn build_app_with_turnstile(
    config: Config,
    clock: Arc<dyn Clock>,
    assets: Arc<dyn StaticAssets>,
    mailer: Arc<dyn crate::mail::Mailer>,
    turnstile: Option<Arc<dyn TurnstileVerifier>>,
) -> Result<App, Error> {
    let pool = open_database(&config).await?;
    let users = UserRepository::new(pool.clone());
    let blog_repo = BlogRepository::new(pool.clone());
    let post_repo = PostRepository::new(pool.clone());

    let cipher = EmailCipher::new(config.email_key, config.email_hmac_key.clone());
    let hasher = PasswordHasher::new(config.argon2_concurrency)?;

    let auth_service = AuthService::new(
        users.clone(),
        SessionRepository::new(pool.clone()),
        VerificationTokenRepository::new(pool.clone()),
        PatRepository::new(pool.clone()),
        crate::auth::cli_repo::CliAuthorizationRepository::new(pool.clone()),
        hasher,
        cipher.clone(),
        mailer.clone(),
        turnstile.clone(),
        clock.clone(),
        config.public_url.clone(),
    );
    let blog_service = BlogService::new(blog_repo.clone(), clock.clone());
    let post_service = PostService::new(
        post_repo,
        blog_repo,
        clock.clone(),
        config.max_posts_per_user,
        config.max_total_content_bytes,
    );

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
        mailer,
        blogs: blog_service,
        posts: post_service,
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
