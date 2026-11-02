//! Entry point.
//!
//! `cairezeki`          start the server
//! `cairezeki migrate`  apply migrations and exit
//! `cairezeki status`   show configuration and pending migrations

use std::process::ExitCode;

use cairezeki::config::Config;
use cairezeki::state::AppState;
use sea_orm_migration::MigratorTrait;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();
    let config = Config::from_env();
    // Held for the life of the process: dropping it flushes and closes the
    // log file, so it must outlive `run`.
    let _log_guard = init_tracing(&config);

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("fatal: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Log to stdout always, and to a daily-rolled file in `config.log_dir` when
/// one is set - by default a `logs/` folder beside the SQLite database, so
/// the volume that holds the data also holds the record of what happened to
/// it. A log directory that cannot be created is reported and skipped; it
/// is never a reason not to start.
fn init_tracing(config: &Config) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_env("CAIREZEKI_LOG")
        .or_else(|_| EnvFilter::try_new("cairezeki=info,tower_http=warn,sea_orm=warn,info"))
        .unwrap_or_default();

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false));

    let Some(dir) = config.log_dir.as_deref() else {
        registry.init();
        return None;
    };

    // Files are named cairezeki.YYYY-MM-DD.log, one per day.
    let appender = std::fs::create_dir_all(dir).map_err(|e| e.to_string()).and_then(|_| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("cairezeki")
            .filename_suffix("log")
            .build(dir)
            .map_err(|e| e.to_string())
    });

    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_target(false)
                        .with_ansi(false)
                        .with_writer(writer),
                )
                .init();
            tracing::info!(dir, "logging to file as well as stdout");
            Some(guard)
        }
        Err(e) => {
            registry.init();
            tracing::warn!(dir, "cannot open log directory ({e}); logging to stdout only");
            None
        }
    }
}

async fn run(config: Config) -> anyhow::Result<()> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "serve".to_string());

    tracing::info!(
        backend = if config.is_sqlite() { "sqlite" } else { "postgres" },
        "connecting to database"
    );
    let db = cairezeki::connect(&config).await?;

    match command.as_str() {
        "migrate" => {
            cairezeki::migrate(&db).await?;
            tracing::info!("migrations applied");
            Ok(())
        }
        "status" => {
            let pending = cairezeki::migration::Migrator::get_pending_migrations(&db).await?;
            println!("database      : {}", redact(&config.database_url));
            println!("bind          : {}", config.bind_addr);
            println!("static dir    : {}", config.static_dir);
            println!("auto-migrate  : {}", config.auto_migrate);
            println!("pending migs  : {}", pending.len());
            Ok(())
        }
        "serve" => serve(config, db).await,
        other => {
            anyhow::bail!("unknown command '{other}' - use: serve, migrate, status")
        }
    }
}

async fn serve(config: Config, db: sea_orm::DatabaseConnection) -> anyhow::Result<()> {
    if config.auto_migrate {
        cairezeki::migrate(&db).await?;
        tracing::info!("schema up to date");
    }

    let bind = config.bind_addr.clone();
    let static_dir = config.static_dir.clone();
    let state = AppState::new(db, config)?;
    cairezeki::auth::ensure_owner(&state).await?;

    let app = cairezeki::api::router(state);

    if !std::path::Path::new(&static_dir).join("index.html").exists() {
        tracing::warn!(
            dir = %static_dir,
            "no index.html found - the API will work but the UI will 404. \
             In development, run the Vite dev server separately."
        );
    }

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("CaiRezekiYojana listening on http://{bind}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shut down cleanly");
    Ok(())
}

/// Containers get SIGTERM; a terminal gets Ctrl-C. Honour both so in-flight
/// requests finish instead of being cut off mid-write.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl-C"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
}

/// Never print a database password into the logs.
fn redact(url: &str) -> String {
    match (url.find("://"), url.find('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end => {
            format!("{}://***{}", &url[..scheme_end], &url[at..])
        }
        _ => url.to_string(),
    }
}
