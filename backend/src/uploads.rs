//! Every imported spreadsheet, kept as uploaded.
//!
//! The database holds what was read out of a file; this keeps the file
//! itself, so an import can be checked against its source or replayed after
//! a fix. Files are named `<UTC timestamp>_<kind>_<original name>`, made
//! read-only, and never overwritten or deleted - every upload is kept, even
//! one that repeats data already imported.
//!
//! Each step - a file validated or imported, an import undone - is also an
//! `upload_audit` row, numbered in order, and a line in `audit.txt` in the
//! same folder, so the sequence can be read without the app.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, Timelike, Utc};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::entities::{app_user, upload_audit};
use crate::error::AppResult;

/// The audit trail in plain text, beside the files it describes.
pub const AUDIT_FILE: &str = "audit.txt";

/// Longest original name kept in a stored file's name. Enough to recognise
/// it; the rest is trimmed.
const MAX_NAME_CHARS: usize = 100;

/// Write `bytes` into `dir` under a timestamped name, creating the folder if
/// need be. Returns the stored file's path.
pub async fn store(
    dir: &str,
    kind: &str,
    filename: &str,
    bytes: &[u8],
    now: DateTime<Utc>,
) -> io::Result<PathBuf> {
    let dir = Path::new(dir);
    tokio::fs::create_dir_all(dir).await?;

    let name = stored_name(kind, filename, now);
    // Two uploads in the same second get a counter rather than one
    // overwriting the other.
    for attempt in 1..=100 {
        let candidate = if attempt == 1 { name.clone() } else { with_counter(&name, attempt) };
        let path = dir.join(&candidate);
        match tokio::fs::OpenOptions::new().write(true).create_new(true).open(&path).await {
            Ok(mut file) => {
                file.write_all(bytes).await?;
                file.flush().await?;
                drop(file);
                make_read_only(&path).await?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(ErrorKind::AlreadyExists, format!("no free name for {name}")))
}

/// A kept file can be read, never written: a stored upload is evidence.
async fn make_read_only(path: &Path) -> io::Result<()> {
    let mut perms = tokio::fs::metadata(path).await?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o444);
    }
    #[cfg(not(unix))]
    perms.set_readonly(true);
    tokio::fs::set_permissions(path, perms).await
}

/// Hex SHA-256 of a file's bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// One audit step, before it has its sequence number.
pub struct AuditEntry {
    /// Whose upload it was.
    pub user_id: i32,
    /// When it happened: now, or the original time when replaying.
    pub at: NaiveDateTime,
    pub action: &'static str,
    pub kind: String,
    pub original_name: String,
    pub stored_as: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub status: String,
    pub batch_id: Option<i32>,
    pub row_count: i32,
    pub ok_count: i32,
    pub skipped_count: i32,
    pub error_count: i32,
    pub message: Option<String>,
}

/// Add a step to the audit: a row in the database, which numbers it, and -
/// when `dir` is given - the same as a line in `audit.txt`. The row is what
/// the app reads; a failure to write the text line is logged rather than
/// undoing a step that has already happened.
pub async fn record(
    db: &DatabaseConnection,
    dir: Option<&str>,
    entry: AuditEntry,
) -> AppResult<upload_audit::Model> {
    let row = upload_audit::ActiveModel {
        user_id: Set(entry.user_id),
        // Whole seconds, as audit.txt records them, so a rebuilt audit matches.
        created_at: Set(entry.at.with_nanosecond(0).unwrap_or(entry.at)),
        action: Set(entry.action.to_string()),
        kind: Set(entry.kind),
        original_name: Set(entry.original_name),
        stored_as: Set(entry.stored_as),
        size_bytes: Set(entry.size_bytes),
        sha256: Set(entry.sha256),
        status: Set(entry.status),
        batch_id: Set(entry.batch_id),
        row_count: Set(entry.row_count),
        ok_count: Set(entry.ok_count),
        skipped_count: Set(entry.skipped_count),
        error_count: Set(entry.error_count),
        message: Set(entry.message),
        ..Default::default()
    }
    .insert(db)
    .await?;

    if let Some(dir) = dir {
        let email = app_user::Entity::find_by_id(row.user_id)
            .one(db)
            .await?
            .map(|u| u.email)
            .unwrap_or_else(|| format!("user-{}", row.user_id));
        if let Err(e) = append_audit_line(dir, &row, &email).await {
            tracing::error!(dir, seq = row.id, "could not write the audit line: {e}");
        }
    }
    Ok(row)
}

async fn append_audit_line(dir: &str, row: &upload_audit::Model, email: &str) -> io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let path = Path::new(dir).join(AUDIT_FILE);
    let fresh = !tokio::fs::try_exists(&path).await.unwrap_or(false);
    let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await?;
    if fresh {
        file.write_all(AUDIT_HEADER.as_bytes()).await?;
    }
    file.write_all(audit_line(row, email).as_bytes()).await?;
    file.flush().await
}

const AUDIT_HEADER: &str = "\
# CaiRezekiYojana upload audit. One line per step, oldest first; lines are only ever
# added. Every file named here is kept read-only in this folder, and its sha256
# lets it be checked against what was uploaded. With an empty database the app
# replays this file on start to rebuild what the uploads put in.
# seq | time (UTC) | user | action | kind | uploaded as | kept as | bytes | sha256 | result
# (Lines from before accounts existed have no user column; they belong to
# the Owner profile.)
";

/// What a text field may not contain without breaking the line apart.
fn flat(s: &str) -> String {
    s.replace(['\n', '\r', '|'], " ")
}

/// `000012 | 2026-09-24 13:45:12 | me@example.com | IMPORT | INDIA_MF | a.xls | 2026..._a.xls | 1234 | ab12.. | COMPLETED: 40 ok, 2 skipped, 0 failed, batch 7`
pub fn audit_line(row: &upload_audit::Model, email: &str) -> String {
    let mut result = format!(
        "{}: {} ok, {} skipped, {} failed",
        row.status, row.ok_count, row.skipped_count, row.error_count
    );
    if let Some(b) = row.batch_id {
        result.push_str(&format!(", batch {b}"));
    }
    if let Some(m) = &row.message {
        result.push_str(&format!(" - {}", flat(m)));
    }
    format!(
        "{:06} | {} | {} | {} | {} | {} | {} | {} | {} | {}\n",
        row.id,
        row.created_at.format(TIME_FORMAT),
        flat(email),
        row.action,
        row.kind,
        flat(&row.original_name),
        row.stored_as.as_deref().unwrap_or("-"),
        row.size_bytes,
        row.sha256.as_deref().unwrap_or("-"),
        result,
    )
}

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// One line of `audit.txt`, read back.
#[derive(Clone, Debug, PartialEq)]
pub struct AuditLine {
    pub seq: i32,
    pub at: NaiveDateTime,
    /// The uploader's email; `None` on a line from before accounts.
    pub user: Option<String>,
    pub action: String,
    pub kind: String,
    pub original_name: String,
    pub stored_as: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    /// The batch the step created or, for an undo, reverted.
    pub batch_id: Option<i32>,
    pub status: String,
    /// Rows imported, skipped and rejected, as the step reported them.
    pub counts: (i32, i32, i32),
}

/// Parse `audit.txt`: comment lines skipped, malformed lines reported.
pub fn parse_audit(text: &str) -> (Vec<AuditLine>, Vec<String>) {
    let mut lines = Vec::new();
    let mut bad = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Some(l) => lines.push(l),
            None => bad.push(format!("line {}: {line}", n + 1)),
        }
    }
    (lines, bad)
}

fn parse_line(line: &str) -> Option<AuditLine> {
    // `flat` keeps `|` out of every field, so the columns split cleanly: ten
    // with the user, nine on a line written before there were accounts.
    let mut f: Vec<&str> = line.split(" | ").map(str::trim).collect();
    let user = match f.len() {
        10 => Some(f.remove(2).to_string()),
        9 => None,
        _ => return None,
    };
    let dash = |s: &str| (s != "-").then(|| s.to_string());
    let result = f[8];
    let head = result.split(" - ").next().unwrap_or("");
    let count = |label: &str| -> i32 {
        head.split(',')
            .find_map(|part| part.trim().rsplit(':').next()?.trim().strip_suffix(label)?.trim().parse().ok())
            .unwrap_or(0)
    };
    let batch_id = result
        .split(" - ")
        .next()
        .and_then(|head| head.rsplit_once(", batch "))
        .and_then(|(_, n)| n.trim().parse().ok());
    Some(AuditLine {
        seq: f[0].parse().ok()?,
        at: NaiveDateTime::parse_from_str(f[1], TIME_FORMAT).ok()?,
        user,
        action: f[2].to_string(),
        kind: f[3].to_string(),
        original_name: f[4].to_string(),
        stored_as: dash(f[5]),
        size_bytes: f[6].parse().ok()?,
        sha256: dash(f[7]),
        batch_id,
        status: result.split(':').next()?.trim().to_string(),
        counts: (count(" ok"), count(" skipped"), count(" failed")),
    })
}

/// `20260924-134512_transactions_Transactions_History_MF.xls`.
pub fn stored_name(kind: &str, filename: &str, now: DateTime<Utc>) -> String {
    format!("{}_{}_{}", now.format("%Y%m%d-%H%M%S"), safe(kind), safe(original_name(filename)))
}

/// The name without any folder a browser sent along with it.
fn original_name(filename: &str) -> &str {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if name.is_empty() {
        "upload"
    } else {
        name
    }
}

/// Letters, digits, `.`, `-` and `_`; anything else becomes `_`. Keeps the
/// name portable and free of path tricks.
fn safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned
    }
}

/// `a_b.xls` -> `a_b-2.xls`.
fn with_counter(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
        None => format!("{name}-{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, 13, 45, 12).unwrap()
    }

    #[test]
    fn names_carry_the_time_the_kind_and_the_original_name() {
        assert_eq!(
            stored_name("TRANSACTIONS", "Transactions_History_MF.xls", at()),
            "20260924-134512_TRANSACTIONS_Transactions_History_MF.xls"
        );
    }

    #[test]
    fn names_are_stripped_of_folders_and_odd_characters() {
        assert_eq!(stored_name("CASH", r"C:\Users\me\bank balances (Sep).xlsx", at()),
            "20260924-134512_CASH_bank_balances__Sep_.xlsx");
        assert_eq!(stored_name("CASH", "../../etc/passwd", at()), "20260924-134512_CASH_passwd");
        assert_eq!(stored_name("CASH", "..", at()), "20260924-134512_CASH_upload");
        assert_eq!(stored_name("CASH", "", at()), "20260924-134512_CASH_upload");
    }

    #[tokio::test]
    async fn a_second_upload_in_the_same_second_does_not_overwrite_the_first() {
        let dir = std::env::temp_dir().join(format!("cairezeki-uploads-{}", std::process::id()));
        let d = dir.to_string_lossy().into_owned();
        let a = store(&d, "CASH", "b.csv", b"one", at()).await.unwrap();
        let b = store(&d, "CASH", "b.csv", b"two", at()).await.unwrap();
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with("_CASH_b-2.csv"));
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_audit_line_reads_back_as_written() {
        let row = upload_audit::Model {
            id: 12,
            user_id: 3,
            created_at: at().naive_utc(),
            action: "IMPORT".into(),
            kind: "INDIA_MF".into(),
            original_name: "My | odd\nname.xls".into(),
            stored_as: Some("20260924-134512_india_mf_My___odd_name.xls".into()),
            size_bytes: 1234,
            sha256: Some("ab".repeat(32)),
            status: "PARTIAL".into(),
            batch_id: Some(7),
            row_count: 43,
            ok_count: 40,
            skipped_count: 2,
            error_count: 1,
            message: Some("row 9 | bad date".into()),
        };
        let text = format!("{AUDIT_HEADER}{}", audit_line(&row, "me@example.com"));
        let (lines, bad) = parse_audit(&text);
        assert!(bad.is_empty());
        assert_eq!(
            lines,
            vec![AuditLine {
                seq: 12,
                at: row.created_at,
                user: Some("me@example.com".into()),
                action: "IMPORT".into(),
                kind: "INDIA_MF".into(),
                original_name: "My   odd name.xls".into(),
                stored_as: row.stored_as.clone(),
                size_bytes: 1234,
                sha256: row.sha256.clone(),
                batch_id: Some(7),
                status: "PARTIAL".into(),
                counts: (40, 2, 1),
            }]
        );
        assert_eq!(parse_audit("000001 | garbage").1.len(), 1);

        // A line from before accounts has no user column, and still reads.
        let legacy = "000003 | 2026-09-24 13:45:12 | IMPORT | GLOBAL | a.xlsx | - | 10 | - | COMPLETED: 1 ok, 0 skipped, 0 failed, batch 2";
        let (old, bad) = parse_audit(legacy);
        assert!(bad.is_empty());
        assert_eq!((old[0].user.clone(), old[0].batch_id, old[0].kind.as_str()), (None, Some(2), "GLOBAL"));
    }

    #[tokio::test]
    async fn kept_files_are_read_only() {
        let dir = std::env::temp_dir().join(format!("cairezeki-ro-{}", std::process::id()));
        let d = dir.to_string_lossy().into_owned();
        let p = store(&d, "CASH", "x.csv", b"1", at()).await.unwrap();
        assert!(std::fs::metadata(&p).unwrap().permissions().readonly());
        std::fs::remove_dir_all(&dir).ok();
    }
}
