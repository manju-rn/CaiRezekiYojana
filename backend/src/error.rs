//! One error type for the whole app, with a sensible HTTP mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),

    /// The caller sent something we cannot act on - bad dates, unknown enum
    /// values, a spreadsheet missing a required column.
    #[error("{0}")]
    BadRequest(String),

    /// The request is well-formed but conflicts with existing data, e.g. a
    /// duplicate transaction reference.
    #[error("{0}")]
    Conflict(String),

    /// No one is signed in, or the session is no longer good - expired,
    /// or the account has since been disabled or locked.
    #[error("{0}")]
    Unauthorized(String),

    /// Signed in, but this is not something this kind of account may do.
    #[error("{0}")]
    Forbidden(String),

    /// Signed in, but the endpoint belongs to another kind of account - a
    /// user calling an administration endpoint, or the other way round.
    /// The body says which roles it is for and which role the caller has.
    #[error("{message}")]
    WrongRole { message: String, required: Vec<&'static str>, role: &'static str },

    /// An upstream feed (quotes, NAV, FX) failed. Never fatal: the app falls
    /// back to the last stored value and says so.
    #[error("provider '{provider}' failed: {message}")]
    Provider { provider: String, message: String },

    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Every error response. `required_roles` and `your_role` are set only for
/// `wrong_role`, so a program can tell "not allowed for this kind of
/// account" from any other refusal.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// Machine-readable: `not_found`, `bad_request`, `conflict`,
    /// `unauthorized`, `forbidden`, `wrong_role`, `provider_error`, ...
    #[schema(example = "wrong_role")]
    pub error: String,
    #[schema(example = "this endpoint is for administrators only")]
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = json!(["ADMIN"]))]
    pub required_roles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "USER")]
    pub your_role: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::WrongRole { .. } => (StatusCode::FORBIDDEN, "wrong_role"),
            AppError::Provider { .. } => (StatusCode::BAD_GATEWAY, "provider_error"),
            AppError::Db(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
            AppError::Http(_) => (StatusCode::BAD_GATEWAY, "upstream_error"),
            AppError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "io_error"),
            AppError::Other(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        // 5xx detail goes to the log, not to the client.
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        }

        let (required_roles, your_role) = match &self {
            AppError::WrongRole { required, role, .. } => {
                (Some(required.iter().map(|r| r.to_string()).collect()), Some(role.to_string()))
            }
            _ => (None, None),
        };
        let body = ErrorBody { error: code.to_string(), message: self.to_string(), required_roles, your_role };
        (status, Json(body)).into_response()
    }
}

impl AppError {
    pub fn not_found(what: &str, id: impl std::fmt::Display) -> Self {
        AppError::NotFound(format!("{what} {id} not found"))
    }

    pub fn bad(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        AppError::Unauthorized(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        AppError::Forbidden(msg.into())
    }

    pub fn wrong_role(msg: impl Into<String>, required: &[crate::auth::Role], role: crate::auth::Role) -> Self {
        AppError::WrongRole {
            message: msg.into(),
            required: required.iter().map(|r| r.as_str()).collect(),
            role: role.as_str(),
        }
    }

    pub fn provider(provider: &str, message: impl Into<String>) -> Self {
        AppError::Provider { provider: provider.to_string(), message: message.into() }
    }
}
