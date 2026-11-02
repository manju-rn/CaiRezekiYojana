//! Shared application state.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};

use crate::config::Config;
use crate::entities::app_setting;
use crate::error::AppResult;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// One shared client so connection pooling and DNS caching actually work.
    pub http: reqwest::Client,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: Config) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
            .user_agent(concat!("cairezeki/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            db,
            http,
            config: Arc::new(config),
        })
    }

    /// Read an app-wide setting from `app_setting`, falling back to
    /// `default`. Each user's own preferences are in `services::prefs`.
    pub async fn setting(&self, key: &str, default: &str) -> String {
        match app_setting::Entity::find_by_id(key.to_string()).one(&self.db).await {
            Ok(Some(row)) if !row.value.is_empty() => row.value,
            _ => default.to_string(),
        }
    }

    pub async fn setting_opt(&self, key: &str) -> Option<String> {
        app_setting::Entity::find_by_id(key.to_string())
            .one(&self.db)
            .await
            .ok()
            .flatten()
            .map(|r| r.value)
            .filter(|v| !v.is_empty())
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let now = chrono::Utc::now().naive_utc();
        let existing = app_setting::Entity::find_by_id(key.to_string()).one(&self.db).await?;
        match existing {
            Some(row) => {
                let mut m: app_setting::ActiveModel = row.into();
                m.value = Set(value.to_string());
                m.updated_at = Set(now);
                m.update(&self.db).await?;
            }
            None => {
                app_setting::ActiveModel {
                    key: Set(key.to_string()),
                    value: Set(value.to_string()),
                    updated_at: Set(now),
                }
                .insert(&self.db)
                .await?;
            }
        }
        Ok(())
    }
}
