//! Uploads with their audit trail, and rebuilding from it.
//!
//! Every upload goes through here: the file is kept (see `crate::uploads`),
//! imported, and the step recorded - whatever the outcome. An undo is a step
//! too. Because the kept files and `audit.txt` hold everything the uploads
//! put in, an empty database can be rebuilt by replaying them in order.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter,
};
use serde::Serialize;

use crate::auth::Role;
use crate::entities::{import_batch, txn, upload_audit};
use crate::error::{AppError, AppResult};
use crate::importer::{self, ImportOutcome};
use crate::state::AppState;
use crate::types::{ImportKind, ImportStatus};
use crate::migration::m20260101_000009_users_and_profiles::OWNER_EMAIL;
use crate::services::users;
use crate::uploads::{self, AuditEntry, AUDIT_FILE};

const VALIDATE: &str = "VALIDATE";
const IMPORT: &str = "IMPORT";
const UNDO: &str = "UNDO";

/// Keep an uploaded file, import it (or only validate it), and record the
/// step. The file is kept before importing so even a failed import can be
/// checked against its source; if it cannot be kept, nothing is imported.
pub async fn upload(
    state: &AppState,
    owner: i32,
    kind: ImportKind,
    filename: &str,
    bytes: &[u8],
    dry_run: bool,
) -> AppResult<ImportOutcome> {
    let dir = state.config.uploads_dir.as_deref();
    let stored_as = match dir {
        Some(dir) => {
            let path = uploads::store(dir, &kind.as_str().to_lowercase(), filename, bytes, Utc::now())
                .await
                .map_err(|e| anyhow::anyhow!("could not save the uploaded file to {dir}: {e}"))?;
            tracing::info!(file = %path.display(), "kept uploaded file");
            path.file_name().map(|n| n.to_string_lossy().into_owned())
        }
        None => None,
    };

    let result = importer::import(&state.db, owner, kind, filename, bytes, dry_run).await;
    let entry = AuditEntry {
        user_id: owner,
        at: Utc::now().naive_utc(),
        action: if dry_run { VALIDATE } else { IMPORT },
        kind: kind.as_str().to_string(),
        original_name: filename.to_string(),
        stored_as: stored_as.clone(),
        size_bytes: bytes.len() as i64,
        sha256: Some(uploads::sha256_hex(bytes)),
        ..outcome_fields(&result)
    };
    let audit = uploads::record(&state.db, dir, entry).await?;

    let mut outcome = result?;
    outcome.stored_as = stored_as;
    outcome.audit_seq = Some(audit.id);
    Ok(outcome)
}

/// The audit fields that come from how an import went.
fn outcome_fields(result: &AppResult<ImportOutcome>) -> AuditEntry {
    let mut e = AuditEntry {
        user_id: 0,
        at: Utc::now().naive_utc(),
        action: IMPORT,
        kind: String::new(),
        original_name: String::new(),
        stored_as: None,
        size_bytes: 0,
        sha256: None,
        status: ImportStatus::Failed.to_string(),
        batch_id: None,
        row_count: 0,
        ok_count: 0,
        skipped_count: 0,
        error_count: 0,
        message: None,
    };
    match result {
        Ok(o) => {
            e.status = if o.dry_run { "VALIDATED".to_string() } else { o.status.clone() };
            e.batch_id = o.batch_id;
            e.row_count = o.row_count as i32;
            e.ok_count = o.ok_count as i32;
            e.skipped_count = o.skipped_count as i32;
            e.error_count = o.error_count as i32;
        }
        Err(err) => e.message = Some(err.to_string()),
    }
    e
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UndoResult {
    pub batch_id: i32,
    pub transactions_removed: u64,
    pub message: String,
}

/// Roll back a transaction import, and record the undo.
///
/// Only transaction batches can be undone: deposits and cash balances are
/// edited in place rather than appended, so there is no clean "before" to
/// return to. The batch record is kept, marked as reverted, and the file
/// stays in the uploads folder - nothing uploaded is ever deleted.
pub async fn undo(state: &AppState, owner: i32, id: i32) -> AppResult<UndoResult> {
    let dir = state.config.uploads_dir.as_deref();
    let (batch, removed) = revert_batch(&state.db, owner, id).await?;
    record_undo(&state.db, dir, &batch, removed, Utc::now().naive_utc()).await?;
    Ok(UndoResult {
        batch_id: id,
        transactions_removed: removed,
        message: format!("removed {removed} transaction(s) from batch {id}"),
    })
}

async fn revert_batch(db: &DatabaseConnection, owner: i32, id: i32) -> AppResult<(import_batch::Model, u64)> {
    let batch = import_batch::Entity::find_by_id(id)
        .filter(import_batch::Column::UserId.eq(owner))
        .one(db)
        .await?
        .ok_or_else(|| AppError::not_found("import batch", id))?;

    let kind = ImportKind::from_str(&batch.kind).map_err(AppError::bad)?;
    if !matches!(kind, ImportKind::IndiaEquity | ImportKind::IndiaMf | ImportKind::Global) {
        return Err(AppError::bad(format!(
            "{} imports cannot be undone automatically - edit or delete the affected records directly",
            batch.kind
        )));
    }

    let removed = txn::Entity::delete_many()
        .filter(txn::Column::ImportBatchId.eq(id))
        .exec(db)
        .await?
        .rows_affected;

    let mut m: import_batch::ActiveModel = batch.clone().into();
    m.status = Set(ImportStatus::Failed.to_string());
    m.message = Set(Some(format!(
        "reverted on {}: {removed} transaction(s) removed",
        Utc::now().date_naive()
    )));
    m.update(db).await?;
    Ok((batch, removed))
}

async fn record_undo(
    db: &DatabaseConnection,
    dir: Option<&str>,
    batch: &import_batch::Model,
    removed: u64,
    at: chrono::NaiveDateTime,
) -> AppResult<upload_audit::Model> {
    // Point at the file of the import being reverted.
    let import = upload_audit::Entity::find()
        .filter(upload_audit::Column::Action.eq(IMPORT))
        .filter(upload_audit::Column::BatchId.eq(batch.id))
        .one(db)
        .await?;
    uploads::record(
        db,
        dir,
        AuditEntry {
            user_id: batch.user_id,
            at,
            action: UNDO,
            kind: batch.kind.clone(),
            original_name: batch.filename.clone(),
            stored_as: import.as_ref().and_then(|i| i.stored_as.clone()),
            size_bytes: import.as_ref().map(|i| i.size_bytes).unwrap_or(0),
            sha256: import.as_ref().and_then(|i| i.sha256.clone()),
            status: "REVERTED".to_string(),
            batch_id: Some(batch.id),
            row_count: 0,
            ok_count: removed as i32,
            skipped_count: 0,
            error_count: 0,
            message: Some(format!("{removed} transaction(s) removed")),
        },
    )
    .await
}

#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct RestoreReport {
    pub steps: usize,
    pub imported: usize,
    pub undone: usize,
    pub warnings: Vec<String>,
}

/// Rebuild from the uploads folder when the database has nothing in it.
///
/// Runs only when there is an `audit.txt` and the database has no upload
/// history and no import batches - a fresh database, not one in use. Each
/// line is replayed in order: an import re-imports its kept file (after
/// checking the file still matches its sha256), an undo reverts the batch
/// that import now has, and a validation is recorded again. The audit rows
/// are recreated with their original times; `audit.txt` itself is left as
/// it is, since it already holds these steps.
pub async fn restore_if_empty(state: &AppState) -> AppResult<Option<RestoreReport>> {
    let Some(dir) = state.config.uploads_dir.as_deref() else { return Ok(None) };
    let path = Path::new(dir).join(AUDIT_FILE);
    let Ok(text) = tokio::fs::read_to_string(&path).await else { return Ok(None) };
    if upload_audit::Entity::find().count(&state.db).await? > 0
        || import_batch::Entity::find().count(&state.db).await? > 0
    {
        return Ok(None);
    }

    let (lines, bad) = uploads::parse_audit(&text);
    if lines.is_empty() {
        return Ok(None);
    }
    tracing::warn!(steps = lines.len(), "empty database with an upload audit - rebuilding from {dir}");

    let db = &state.db;
    let mut report = RestoreReport { steps: lines.len(), ..Default::default() };
    report.warnings.extend(bad.into_iter().map(|b| format!("unreadable audit line skipped: {b}")));
    // Batch ids in the audit, to the batches the replay created.
    let mut batches: HashMap<i32, i32> = HashMap::new();
    let mut owners: HashMap<String, i32> = HashMap::new();

    for line in lines {
        let owner = match replay_owner(state, &mut owners, line.user.as_deref()).await {
            Ok(id) => id,
            Err(e) => {
                report.warnings.push(format!("#{}: no account for its uploader: {e}", line.seq));
                continue;
            }
        };
        let row = match line.action.as_str() {
            UNDO => {
                let Some(new_id) = line.batch_id.and_then(|old| batches.get(&old).copied()) else {
                    report.warnings.push(format!("#{}: undo of a batch that was not rebuilt", line.seq));
                    continue;
                };
                match revert_batch(db, owner, new_id).await {
                    Ok((batch, removed)) => {
                        report.undone += 1;
                        record_undo(db, None, &batch, removed, line.at).await?
                    }
                    Err(e) => {
                        report.warnings.push(format!("#{}: undo failed: {e}", line.seq));
                        continue;
                    }
                }
            }
            IMPORT | VALIDATE => {
                let dry_run = line.action == VALIDATE;
                let file = match read_kept(dir, &line).await {
                    Ok(bytes) => bytes,
                    Err(why) => {
                        report.warnings.push(format!("#{}: {why}", line.seq));
                        continue;
                    }
                };
                let Ok(kind) = ImportKind::from_str(&line.kind) else {
                    report.warnings.push(format!("#{}: unknown kind {}", line.seq, line.kind));
                    continue;
                };
                // A validation imports nothing, so there is nothing to redo.
                let result = if dry_run || line.status == ImportStatus::Failed.as_str() {
                    None
                } else {
                    Some(importer::import(db, owner, kind, &line.original_name, &file, false).await)
                };
                if let Some(Ok(o)) = &result {
                    report.imported += 1;
                    if let (Some(old), Some(new)) = (line.batch_id, o.batch_id) {
                        batches.insert(old, new);
                    }
                }
                if let Some(Err(e)) = &result {
                    report.warnings.push(format!("#{}: import failed on replay: {e}", line.seq));
                }
                let mut entry = match &result {
                    Some(r) => outcome_fields(r),
                    None => outcome_fields(&Err(AppError::bad("not replayed"))),
                };
                if result.is_none() {
                    // Recorded as it was the first time.
                    entry.status = line.status.clone();
                    entry.message = None;
                    (entry.ok_count, entry.skipped_count, entry.error_count) = line.counts;
                }
                entry.user_id = owner;
                entry.at = line.at;
                entry.action = if dry_run { VALIDATE } else { IMPORT };
                entry.kind = line.kind.clone();
                entry.original_name = line.original_name.clone();
                entry.stored_as = line.stored_as.clone();
                entry.size_bytes = line.size_bytes;
                entry.sha256 = line.sha256.clone();
                uploads::record(db, None, entry).await?
            }
            other => {
                report.warnings.push(format!("#{}: unknown action {other}", line.seq));
                continue;
            }
        };
        if row.id != line.seq {
            report.warnings.push(format!("audit #{} was recreated as #{}", line.seq, row.id));
        }
    }

    for w in &report.warnings {
        tracing::warn!("restore: {w}");
    }
    tracing::info!(
        imported = report.imported,
        undone = report.undone,
        "rebuilt from the upload audit"
    );
    Ok(Some(report))
}


/// The account a replayed line belongs to, by the uploader's email - the
/// Owner profile for a line from before accounts. An account that does not
/// exist yet (the database is empty, after all) is created without a
/// password, for an administrator to issue an activation token to.
async fn replay_owner(state: &AppState, cache: &mut HashMap<String, i32>, email: Option<&str>) -> AppResult<i32> {
    let email = email.unwrap_or(OWNER_EMAIL).to_lowercase();
    if let Some(id) = cache.get(&email) {
        return Ok(*id);
    }
    let id = match users::find_by_email(&state.db, &email).await? {
        Some(u) => u.id,
        None => {
            let name = if email == OWNER_EMAIL { "Owner" } else { "" };
            let created = users::create(
                state,
                users::NewUser {
                    email: &email,
                    display_name: name,
                    role: Role::User,
                    password: None,
                    email_verified: true,
                    must_set_password: true,
                    awaiting_approval: false,
                    base_currency: None,
                },
            )
            .await?;
            tracing::warn!(email, "restore: created an account to hold its uploads - issue it an activation token");
            created.id
        }
    };
    cache.insert(email, id);
    Ok(id)
}
/// A kept file's bytes, checked against the fingerprint the audit holds.
async fn read_kept(dir: &str, line: &uploads::AuditLine) -> Result<Vec<u8>, String> {
    let name = line.stored_as.as_deref().ok_or("no kept file recorded")?;
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(format!("refusing an odd file name: {name}"));
    }
    let bytes = tokio::fs::read(Path::new(dir).join(name))
        .await
        .map_err(|e| format!("kept file {name} could not be read: {e}"))?;
    if let Some(expected) = &line.sha256 {
        if &uploads::sha256_hex(&bytes) != expected {
            return Err(format!("kept file {name} no longer matches its sha256 - not replayed"));
        }
    }
    Ok(bytes)
}

/// `owner`'s uploads, newest first.
pub async fn list(db: &DatabaseConnection, owner: i32, limit: u64) -> AppResult<Vec<upload_audit::Model>> {
    use sea_orm::{QueryOrder, QuerySelect};
    Ok(upload_audit::Entity::find()
        .filter(upload_audit::Column::UserId.eq(owner))
        .order_by_desc(upload_audit::Column::Id)
        .limit(limit)
        .all(db)
        .await?)
}

/// A kept file, for download: its name and bytes.
pub async fn kept_file(state: &AppState, owner: i32, seq: i32) -> AppResult<(String, Vec<u8>)> {
    let row = upload_audit::Entity::find_by_id(seq)
        .filter(upload_audit::Column::UserId.eq(owner))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("audit step", seq))?;
    let dir = state
        .config
        .uploads_dir
        .as_deref()
        .ok_or_else(|| AppError::bad("no uploads folder is configured"))?;
    let name = row.stored_as.ok_or_else(|| AppError::bad("no file was kept for this step"))?;
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(AppError::bad("odd file name in the audit"));
    }
    let bytes = tokio::fs::read(Path::new(dir).join(&name)).await?;
    Ok((name, bytes))
}
