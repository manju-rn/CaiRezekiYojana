//! Spreadsheet upload, templates, and batch history.

use axum::body::Body;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;

use crate::auth::Owner;
use crate::entities::{import_batch, upload_audit};
use crate::error::{AppError, AppResult};
use crate::importer::portfolio::PORTFOLIO;
use crate::importer::{ImportOutcome, ImportSpec};
use crate::services::{audit, portfolio_import};
use crate::state::AppState;

use super::openapi::tags;

/// The one accepted format, for the Import screen's documentation panel.
#[utoipa::path(
    get,
    path = "/imports/formats",
    operation_id = "list_import_formats",
    tag = tags::USER_IMPORTS,
    summary = "The accepted portfolio format and its columns",
    responses((status = 200, body = [ImportSpec]))
)]
pub async fn formats(_: Owner) -> Json<Vec<&'static ImportSpec>> {
    Json(vec![&PORTFOLIO])
}

/// The only import there is: `portfolio`, with or without `.csv` on the end.
fn check_kind(raw: &str) -> AppResult<()> {
    let cleaned = raw.trim_end_matches(".csv").trim_end_matches(".xlsx");
    if cleaned.eq_ignore_ascii_case("portfolio") {
        Ok(())
    } else {
        Err(AppError::bad(format!("unknown import kind '{raw}' - the only one is: portfolio")))
    }
}

/// Download the template for the portfolio format.
///
/// Served as CSV: Excel opens it directly and "Save As .xlsx" takes one
/// click, which is a better trade than shipping a spreadsheet writer in the
/// binary purely to pre-format a header row.
#[utoipa::path(
    get,
    path = "/imports/templates/{kind}",
    operation_id = "get_import_template",
    tag = tags::USER_IMPORTS,
    summary = "Download the CSV template for the portfolio format",
    params(("kind" = String, Path, description = "`portfolio`; a trailing `.csv` is allowed")),
    responses(
        (status = 200, description = "The template", content_type = "text/csv", body = String),
        (status = 400, description = "Unknown kind", body = crate::error::ErrorBody),
    )
)]
pub async fn template(_: Owner, Path(kind): Path<String>) -> AppResult<Response> {
    check_kind(&kind)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"cairezeki-portfolio-template.csv\"".to_string(),
            ),
        ],
        Body::from(PORTFOLIO.template_csv()),
    )
        .into_response())
}

/// The multipart body of an upload. Only for the API description.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct UploadForm {
    /// The spreadsheet: `.xlsx`, `.xls` or `.csv`.
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UploadParams {
    /// Validate and report without writing anything.
    pub dry_run: Option<bool>,
}

/// Upload a spreadsheet. Field name is `file`; anything else is ignored.
#[utoipa::path(
    post,
    path = "/imports/{kind}",
    operation_id = "upload_import",
    tag = tags::USER_IMPORTS,
    summary = "Upload a portfolio CSV: every holding in one file",
    description = "Multipart, with the file in the field `file`. Up to 32 MB. With `dry_run=true` the file is validated and reported on but nothing is written.",
    params(("kind" = String, Path, description = "`portfolio`"), UploadParams),
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, body = ImportOutcome),
        (status = 400, description = "Unknown kind, no file, or a file that could not be read", body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
    )
)]
pub async fn upload(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(kind): Path<String>,
    Query(params): Query<UploadParams>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Json<ImportOutcome>> {
    // The kind first: an unknown one says so, whatever was sent with it.
    check_kind(&kind)?;
    let mut multipart = multipart.map_err(|e| AppError::bad(format!("malformed upload: {}", e.body_text())))?;

    let mut filename = String::new();
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad(format!("malformed upload: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        filename = field.file_name().unwrap_or("upload.xlsx").to_string();
        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::bad(format!("could not read the uploaded file: {e}")))?;
        bytes = Some(data.to_vec());
        break;
    }

    let bytes = bytes.ok_or_else(|| {
        AppError::bad("no file in the request - send it as multipart form field 'file'")
    })?;
    if bytes.is_empty() {
        return Err(AppError::bad("the uploaded file is empty"));
    }
    if bytes.len() > super::MAX_UPLOAD_BYTES {
        return Err(AppError::bad(format!(
            "file is {} MB; the limit is {} MB",
            bytes.len() / 1_048_576,
            super::MAX_UPLOAD_BYTES / 1_048_576
        )));
    }

    let outcome =
        portfolio_import::upload(&state, owner, &filename, &bytes, params.dry_run.unwrap_or(false)).await?;
    Ok(Json(outcome))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BatchParams {
    /// Newest first; default 50, at most 500.
    pub limit: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/imports",
    operation_id = "list_import_batches",
    tag = tags::USER_IMPORTS,
    summary = "The caller's import batches, newest first",
    params(BatchParams),
    responses((status = 200, body = [import_batch::Model]))
)]
pub async fn list_batches(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(params): Query<BatchParams>,
) -> AppResult<Json<Vec<import_batch::Model>>> {
    Ok(Json(
        import_batch::Entity::find()
            .filter(import_batch::Column::UserId.eq(owner))
            .order_by_desc(import_batch::Column::CreatedAt)
            .limit(params.limit.unwrap_or(50).min(500))
            .all(&state.db)
            .await?,
    ))
}

/// Roll back a transaction import; see `services::audit::undo`.
#[utoipa::path(
    delete,
    path = "/imports/{id}/undo",
    operation_id = "undo_import_batch",
    tag = tags::USER_IMPORTS,
    summary = "Roll back an import batch",
    params(("id" = i32, Path, description = "Batch id")),
    responses((status = 200, body = audit::UndoResult), (status = 400, body = crate::error::ErrorBody), (status = 404, body = crate::error::ErrorBody), (status = 409, body = crate::error::ErrorBody))
)]
pub async fn undo_batch(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<audit::UndoResult>> {
    Ok(Json(audit::undo(&state, owner, id).await?))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditParams {
    /// Newest first; default 200, at most 5000.
    pub limit: Option<u64>,
}

/// The upload audit, newest first.
#[utoipa::path(
    get,
    path = "/uploads",
    operation_id = "list_uploads",
    tag = tags::USER_IMPORTS,
    summary = "The caller's upload audit, newest first",
    params(AuditParams),
    responses((status = 200, body = [upload_audit::Model]))
)]
pub async fn list_audit(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(params): Query<AuditParams>,
) -> AppResult<Json<Vec<upload_audit::Model>>> {
    Ok(Json(audit::list(&state.db, owner, params.limit.unwrap_or(200).min(5000)).await?))
}

/// A kept upload, exactly as it was uploaded.
#[utoipa::path(
    get,
    path = "/uploads/{seq}/file",
    operation_id = "download_upload",
    tag = tags::USER_IMPORTS,
    summary = "Download a kept upload exactly as it was uploaded",
    params(("seq" = i32, Path, description = "The upload's sequence number")),
    responses(
        (status = 200, description = "The file", content_type = "application/octet-stream", body = Vec<u8>),
        (status = 404, body = crate::error::ErrorBody),
    )
)]
pub async fn kept_file(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(seq): Path<i32>,
) -> AppResult<Response> {
    let (name, bytes) = audit::kept_file(&state, owner, seq).await?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))
        .body(Body::from(bytes))
        .map_err(|e| anyhow::anyhow!(e))?)
}
