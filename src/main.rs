use std::sync::Arc;
use std::time::Duration;

use statesu::bootstrap::build_app;
use statesu::clock::SystemClock;
use statesu::config::Config;
use statesu::http::static_files::EmbeddedDocs;

fn main() {
    if let Err(err) = run() {
        // Startup errors fail fast with one line; secrets are never printed.
        eprintln!("statesu: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // .env is optional in production; explicit environment always wins.
    if std::path::Path::new(".env").exists() {
        dotenvy::dotenv()?;
    }
    let config = Config::from_env()?;
    install_tracing(config.log_json);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(config))?;
    runtime.shutdown_timeout(Duration::from_secs(10));
    Ok(())
}

async fn serve(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(SystemClock);
    let assets = Arc::new(EmbeddedDocs);
    let app = build_app(config, clock, assets).await?;

    let listener = tokio::net::TcpListener::bind(&app.state.config.addr).await?;
    tracing::info!(addr = %app.state.config.addr, "statesu listening");

    let server = axum::serve(
        listener,
        app.router
            .clone()
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());
    if let Err(err) = server.await {
        tracing::error!(error = %err, "server error");
    }
    // Drain idle pool connections so SQLite checkpoints cleanly on close.
    app.close_database().await;
    Ok(())
}

fn install_tracing(log_json: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr);
    if log_json {
        registry.json().init();
    } else {
        registry.init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = ctrl_c => {},
                    _ = sigterm.recv() => {},
                }
            }
            Err(err) => {
                // Without SIGTERM support the container would still stop on
                // SIGKILL, but installing a signal handler is expected to work.
                tracing::error!(error = %err, "failed to install SIGTERM handler, relying on ctrl_c only");
                let _ = ctrl_c.await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
