//! Uploading the single portfolio CSV: sorted into the importer's formats
//! (see [`crate::importer::portfolio`]), each applied and audited as an
//! upload of its own, and answered as one.

use std::collections::BTreeSet;

use chrono::Utc;

use crate::error::AppResult;
use crate::importer::portfolio::{self, KIND};
use crate::importer::{ImportOutcome, RowError};
use crate::services::audit;
use crate::state::AppState;
use crate::types::ImportStatus;

/// Import (or, with `dry_run`, only validate) a portfolio CSV for `owner`.
pub async fn upload(
    state: &AppState,
    owner: i32,
    filename: &str,
    bytes: &[u8],
    dry_run: bool,
) -> AppResult<ImportOutcome> {
    let split = portfolio::split(filename, bytes, Utc::now().date_naive())?;
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);

    let mut errors: Vec<RowError> = split.errors;
    let mut batch_id = None;
    let mut stored_as = None;
    let mut audit_seq = None;
    let (mut ok_count, mut skipped_count) = (0usize, 0usize);
    let mut accounts = BTreeSet::new();
    let mut instruments = BTreeSet::new();

    for part in split.parts {
        let name = format!("{stem}.{}.csv", part.kind.as_str().to_lowercase());
        let o = audit::upload(state, owner, part.kind, &name, part.csv.as_bytes(), dry_run).await?;
        ok_count += o.ok_count;
        skipped_count += o.skipped_count;
        errors.extend(o.errors);
        accounts.extend(o.created_accounts);
        instruments.extend(o.created_instruments);
        batch_id = batch_id.or(o.batch_id);
        stored_as = stored_as.or(o.stored_as);
        audit_seq = audit_seq.or(o.audit_seq);
    }
    errors.sort_by_key(|e| e.row);

    let status = if !errors.is_empty() && ok_count == 0 {
        ImportStatus::Failed
    } else if !errors.is_empty() {
        ImportStatus::Partial
    } else {
        ImportStatus::Completed
    };
    Ok(ImportOutcome {
        batch_id,
        kind: KIND.to_string(),
        filename: filename.to_string(),
        status: status.to_string(),
        row_count: split.row_count,
        ok_count,
        skipped_count,
        error_count: errors.len(),
        errors,
        created_accounts: accounts.into_iter().collect(),
        created_instruments: instruments.into_iter().collect(),
        unrecognised_columns: split.unrecognised_columns,
        dry_run,
        stored_as,
        audit_seq,
    })
}
