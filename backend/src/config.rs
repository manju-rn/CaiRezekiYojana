//! Runtime configuration, entirely from the environment so the same image
//! runs in Docker, on k8s, or from `cargo run`.

use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    /// `sqlite://...` or `postgres://...` - the app adapts either way.
    pub database_url: String,
    pub bind_addr: String,
    /// Directory holding the built React app. Served as a SPA fallback.
    pub static_dir: String,
    /// Run pending migrations on boot. On by default; turn it off if you
    /// prefer to gate schema changes behind a separate job.
    pub auto_migrate: bool,
    /// Extra origins allowed by CORS. The bundled UI is same-origin and does
    /// not need this; a separately hosted frontend does.
    pub cors_origins: Vec<String>,
    pub request_timeout_secs: u64,
    /// Where daily-rolled log files go, in addition to stdout. Defaults to a
    /// `logs/` folder beside the SQLite file; `CAIREZEKI_LOG_DIR=none` turns
    /// file logging off. Postgres deployments log to stdout only unless this
    /// is set.
    pub log_dir: Option<String>,
    /// Where every imported spreadsheet is kept, timestamped, as uploaded.
    /// Defaults to an `uploads/` folder beside the SQLite file;
    /// `CAIREZEKI_UPLOADS_DIR=none` keeps nothing. Postgres deployments keep
    /// nothing unless this is set.
    pub uploads_dir: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("CAIREZEKI_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://./data/cairezeki.db?mode=rwc".to_string());
        let log_dir = dir_setting("CAIREZEKI_LOG_DIR", &database_url, "logs");
        let uploads_dir = dir_setting("CAIREZEKI_UPLOADS_DIR", &database_url, "uploads");

        Self {
            database_url,
            bind_addr: env::var("CAIREZEKI_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            static_dir: env::var("CAIREZEKI_STATIC_DIR").unwrap_or_else(|_| "./static".to_string()),
            auto_migrate: env_bool("CAIREZEKI_AUTO_MIGRATE", true),
            cors_origins: env::var("CAIREZEKI_CORS_ORIGINS")
                .ok()
                .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
            request_timeout_secs: env::var("CAIREZEKI_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            log_dir,
            uploads_dir,
        }
    }

    pub fn is_sqlite(&self) -> bool {
        self.database_url.starts_with("sqlite")
    }
}

/// The file behind a SQLite URL: `sqlite:///data/cairezeki.db?mode=rwc` ->
/// `/data/cairezeki.db`. `None` for Postgres and for `:memory:`.
pub fn sqlite_file_path(url: &str) -> Option<&str> {
    if !url.starts_with("sqlite") {
        return None;
    }
    let path = url
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:")
        .split('?')
        .next()
        .unwrap_or("");
    if path.is_empty() || path == ":memory:" {
        None
    } else {
        Some(path)
    }
}

/// `<directory of the SQLite file>/<name>`, so logs and temp files live on
/// the same volume as the data - the one place a container is sure to be
/// able to write.
pub fn sqlite_sibling_dir(url: &str, name: &str) -> Option<String> {
    let file = std::path::Path::new(sqlite_file_path(url)?);
    let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = match parent {
        Some(p) => p.join(name),
        None => std::path::PathBuf::from(name),
    };
    Some(dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_paths_come_out_of_the_url() {
        assert_eq!(sqlite_file_path("sqlite:///data/cairezeki.db?mode=rwc"), Some("/data/cairezeki.db"));
        assert_eq!(sqlite_file_path("sqlite://./data/x.db"), Some("./data/x.db"));
        assert_eq!(sqlite_file_path("sqlite::memory:"), None);
        assert_eq!(sqlite_file_path("postgres://localhost/x"), None);
    }

    #[test]
    fn sibling_directories_sit_beside_the_database() {
        assert_eq!(sqlite_sibling_dir("sqlite:///data/cairezeki.db?mode=rwc", "logs").as_deref(), Some("/data/logs"));
        assert_eq!(sqlite_sibling_dir("sqlite://cairezeki.db", "tmp").as_deref(), Some("tmp"));
        assert_eq!(sqlite_sibling_dir("postgres://localhost/x", "logs"), None);
    }
}

/// A folder setting: the variable if set, `none` (or `-`) for no folder,
/// otherwise `<name>` beside the SQLite file.
fn dir_setting(key: &str, database_url: &str, name: &str) -> Option<String> {
    match env::var(key).ok().map(|v| v.trim().to_string()) {
        Some(v) if v.eq_ignore_ascii_case("none") || v == "-" => None,
        Some(v) if !v.is_empty() => Some(v),
        _ => sqlite_sibling_dir(database_url, name),
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}
