//! HTTP surface.
//!
//! Everything the UI can do is an API call, and the API is the contract -
//! there is no private back channel. That is what makes the app scriptable:
//! a cron job or a shell script can drive it exactly as the browser does.

pub mod cash;
pub mod dashboard;
pub mod data;
pub mod deposits;
pub mod dhan_yatra;
pub mod holdings;
pub mod imports;
pub mod meta;
pub mod openapi;
pub mod planner;
pub mod plan_shares;
pub mod retirement;
pub mod settings;

use axum::extract::DefaultBodyLimit;
use std::sync::Arc;

use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware;

use axum::extract::{OriginalUri, Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_status::SetStatus;
use tower_http::trace::TraceLayer;

use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

use crate::error::AppError;
use crate::state::AppState;

/// Upload cap. Comfortably larger than any portfolio file, small enough that
/// a stray file cannot exhaust memory.
pub const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    let (api, mut spec) = documented().split_for_parts();
    openapi::stamp_version(&mut spec);
    let spec = Arc::new(serde_json::to_value(&spec).expect("the description serialises"));

    let api = api.route("/openapi.json", get(described).layer(Extension(spec)));

    // An unknown path under the API is a JSON 404, not the web app's page;
    // and every failure is the same JSON shape, whoever answered it.
    let api = api.fallback(api_not_found).layer(middleware::map_response(json_errors));

    Router::new()
        .nest("/api/v1", api)
        .merge(swagger())
        .fallback(web)
        .layer(cors(&state.config.cors_origins))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// `404 {"error": "not_found"}` for a path the API does not have. A known
/// path with the wrong method stays a `405`, which the router answers itself.
async fn api_not_found(OriginalUri(uri): OriginalUri) -> AppError {
    AppError::NotFound(format!("no endpoint at {}", uri.path()))
}

/// Every error under `/api/v1` as `{"error", "message"}` JSON. The handlers'
/// own errors already are; this catches the framework's - a body or query
/// that does not parse (plain text), a wrong method (empty) - so a client
/// can always read an error the same way. A body that does not fit the
/// endpoint's fields is a `400`, as the handlers' own checks are, not `422`.
async fn json_errors(res: Response) -> Response {
    let status = res.status();
    let is_json = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !(status.is_client_error() || status.is_server_error()) || is_json {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let text = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let status = if status == StatusCode::UNPROCESSABLE_ENTITY { StatusCode::BAD_REQUEST } else { status };
    let code = match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::PAYLOAD_TOO_LARGE => "too_large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        s if s.is_server_error() => "internal_error",
        _ => "error",
    };
    let message = if text.is_empty() { status.canonical_reason().unwrap_or("Error").to_string() } else { text };
    parts.status = status;
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let body = serde_json::json!({ "error": code, "message": message });
    Response::from_parts(parts, axum::body::Body::from(body.to_string()))
}

/// The whole description, with the release stamped on it.
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut spec = documented().split_for_parts().1;
    openapi::stamp_version(&mut spec);
    spec
}

/// `/api/v1/openapi.json`.
async fn described(Extension(spec): Extension<Arc<serde_json::Value>>) -> impl IntoResponse {
    Json(spec.as_ref().clone())
}

/// The Swagger page at `/api/docs`, reading `/api/v1/openapi.json`.
fn swagger() -> Router<AppState> {
    Router::from(SwaggerUi::new("/api/docs").config(utoipa_swagger_ui::Config::from("/api/v1/openapi.json")))
}

/// Every route that is part of the published API, each from its handler's
/// `#[utoipa::path]`. Handlers sharing a path go in one `routes!`.
fn documented() -> OpenApiRouter<AppState> {
    let uploads = OpenApiRouter::new()
        .routes(routes!(imports::upload))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES));

    OpenApiRouter::with_openapi(openapi::ApiDoc::openapi())
        // -- public: health and reference data ------------------------------
        .routes(routes!(meta::health))
        .routes(routes!(meta::list_currencies))
        .routes(routes!(meta::list_exchanges))
        .routes(routes!(meta::list_enums))
        // -- settings -------------------------------------------------------
        .routes(routes!(settings::list, settings::update))
        .routes(routes!(data::inventory))
        .routes(routes!(data::delete))
        // -- holdings -------------------------------------------------------
        .routes(routes!(holdings::list))
        // -- deposits -------------------------------------------------------
        .routes(routes!(deposits::list, deposits::create))
        .routes(routes!(deposits::summary))
        .routes(routes!(deposits::get_one, deposits::update, deposits::remove))
        // -- cash -----------------------------------------------------------
        .routes(routes!(cash::list))
        .routes(routes!(cash::list_balances, cash::record_balance))
        .routes(routes!(cash::delete_balance))
        // -- imports --------------------------------------------------------
        .routes(routes!(imports::list_batches))
        .routes(routes!(imports::formats))
        .routes(routes!(imports::template))
        .merge(uploads)
        .routes(routes!(imports::undo_batch))
        .routes(routes!(imports::list_audit))
        .routes(routes!(imports::kept_file))
        // -- net worth ------------------------------------------------------
        .routes(routes!(dashboard::net_worth))
        .routes(routes!(dashboard::refresh_all))
        // -- retirement planning and CaiYatra -------------------------------
        .routes(routes!(retirement::simulate))
        .routes(routes!(dhan_yatra::simulate))
        .routes(routes!(dhan_yatra::list_sessions, dhan_yatra::save_session))
        .routes(routes!(dhan_yatra::import_session))
        .routes(routes!(dhan_yatra::get_session, dhan_yatra::delete_session))
        .routes(routes!(dhan_yatra::export_session))
        // -- RezekiYojana: allocation plans ----------------------------------
        .routes(routes!(planner::list_sessions, planner::save_session))
        .routes(routes!(planner::import_session))
        .routes(routes!(planner::get_session, planner::delete_session))
        .routes(routes!(planner::export_session))
        .routes(routes!(plan_shares::list, plan_shares::create))
        .routes(routes!(plan_shares::update, plan_shares::revoke))
        .routes(routes!(plan_shares::shared_with_me))
        .routes(routes!(plan_shares::get_shared_with_me))
        // -- a shared link: the planner alone, read-only ---------------------
        .routes(routes!(plan_shares::open_link))
        .routes(routes!(plan_shares::link_plan))
        .routes(routes!(planner::get, planner::replace, planner::reset))
        .routes(routes!(planner::add_move))
        .routes(routes!(planner::remove_move))
        .routes(routes!(planner::add_category))
        .routes(routes!(planner::remove_category))
        .routes(routes!(planner::restore_category))
        .routes(routes!(planner::set_class))
        .routes(routes!(planner::add_sub))
        .routes(routes!(planner::remove_sub))
        .routes(routes!(planner::set_fresh))
}

/// Everything outside the API: the React app. A file that is not there - a
/// stale asset after a deploy - is a real 404; any other path is the app's
/// page, so client-side routes survive a hard refresh or a shared link.
async fn web(State(state): State<AppState>, req: Request) -> Response {
    let dir = &state.config.static_dir;
    let path = req.uri().path();
    let res = if path.rsplit('/').next().is_some_and(|last| last.contains('.')) {
        spa_files(dir).oneshot(req).await.map(IntoResponse::into_response)
    } else {
        spa(dir).oneshot(req).await.map(IntoResponse::into_response)
    };
    match res {
        Ok(res) => res,
        Err(never) => match never {},
    }
}

/// Serve the built React app, falling back to `index.html` - with a 200, not
/// a 404 - so client-side routes survive a hard refresh or a shared link.
fn spa(dir: &str) -> ServeDir<ServeFile> {
    ServeDir::new(dir).fallback(ServeFile::new(format!("{dir}/index.html")))
}

/// The app's files alone: a missing one is a 404 with the app's page.
fn spa_files(dir: &str) -> ServeDir<SetStatus<ServeFile>> {
    ServeDir::new(dir).not_found_service(ServeFile::new(format!("{dir}/index.html")))
}

fn cors(origins: &[String]) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    if origins.is_empty() {
        // The bundled UI is same-origin, so nothing extra needs allowing.
        layer
    } else {
        let parsed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
        layer.allow_origin(parsed)
    }
}

/// What the delete endpoints return.
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct Deleted {
    pub deleted: bool,
    pub id: i32,
}
