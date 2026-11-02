//! Reading uploaded spreadsheets into plain strings.
//!
//! Everything is normalised to text here and parsed later, which keeps the
//! xlsx and csv paths identical and makes the parsers testable without a
//! spreadsheet library in the loop.

use std::collections::HashMap;
use std::io::Cursor;

use chrono::{Datelike, NaiveDate, NaiveTime};

use crate::error::{AppError, AppResult};

/// A sheet flattened to headers plus string rows.
#[derive(Debug, Clone)]
pub struct Sheet {
    /// Normalised header keys, in column order.
    pub headers: Vec<String>,
    pub rows: Vec<Row>,
}

/// One data row, keyed by normalised header.
#[derive(Debug, Clone)]
pub struct Row {
    /// 1-based row number *in the original file*, so error messages point at
    /// something the user can actually find in Excel.
    pub number: usize,
    pub cells: HashMap<String, String>,
}

impl Row {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.cells.get(key).map(|s| s.as_str()).filter(|s| !s.is_empty())
    }

    pub fn is_blank(&self) -> bool {
        self.cells.values().all(|v| v.trim().is_empty())
    }
}

/// `"Trade Date "` -> `trade_date`. Everything non-alphanumeric collapses to
/// a single underscore so `Qty.`, `qty`, and `QTY` all land on `qty`.
pub fn normalise_header(raw: &str) -> String {
    let lowered = raw.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_sep = true;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Dispatch on extension: xlsx/xlsm/xls/ods go through calamine, everything
/// else is treated as delimited text.
pub fn read(filename: &str, bytes: &[u8]) -> AppResult<Sheet> {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => read_workbook(bytes),
        "csv" | "txt" | "tsv" => read_delimited(bytes, if ext == "tsv" { b'\t' } else { b',' }),
        other => Err(AppError::bad(format!(
            "unsupported file type '.{other}' - upload .xlsx, .xls, .ods or .csv"
        ))),
    }
}

fn read_workbook(bytes: &[u8]) -> AppResult<Sheet> {
    use calamine::Reader;

    let mut workbook = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| AppError::bad(format!("could not open workbook: {e}")))?;

    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| AppError::bad("workbook contains no sheets"))?;

    let range = workbook
        .worksheet_range(&name)
        .map_err(|e| AppError::bad(format!("could not read sheet '{name}': {e}")))?;

    let mut iter = range.rows();
    let header_row = iter
        .next()
        .ok_or_else(|| AppError::bad("sheet is empty - the first row must hold column headers"))?;

    let headers: Vec<String> = header_row.iter().map(|c| normalise_header(&c.to_string())).collect();

    let mut rows = Vec::new();
    for (i, raw) in iter.enumerate() {
        let mut cells = HashMap::new();
        for (idx, cell) in raw.iter().enumerate() {
            let Some(key) = headers.get(idx) else { continue };
            if key.is_empty() {
                continue;
            }
            cells.insert(key.clone(), cell.to_string().trim().to_string());
        }
        // +2: one for the header row, one to get from 0- to 1-based.
        let row = Row { number: i + 2, cells };
        if !row.is_blank() {
            rows.push(row);
        }
    }

    Ok(Sheet { headers, rows })
}

fn read_delimited(bytes: &[u8], delimiter: u8) -> AppResult<Sheet> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| AppError::bad(format!("could not read the header row: {e}")))?
        .iter()
        .map(normalise_header)
        .collect();

    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| AppError::bad(format!("row {}: {e}", i + 2)))?;
        let mut cells = HashMap::new();
        for (idx, value) in record.iter().enumerate() {
            let Some(key) = headers.get(idx) else { continue };
            if key.is_empty() {
                continue;
            }
            cells.insert(key.clone(), value.trim().to_string());
        }
        let row = Row { number: i + 2, cells };
        if !row.is_blank() {
            rows.push(row);
        }
    }

    Ok(Sheet { headers, rows })
}

// ---------------------------------------------------------------------------
// Value parsing
// ---------------------------------------------------------------------------

/// Excel counts days from 1899-12-30 - the offset that reproduces its
/// deliberate 1900-leap-year bug for every date after February 1900.
fn excel_serial_to_date(serial: f64) -> Option<NaiveDate> {
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    epoch.checked_add_signed(chrono::Duration::days(serial.trunc() as i64))
}

/// Formats accepted in a date column.
///
/// Day-first is tried before month-first: India, Singapore and Australia all
/// write 03/04/2026 as 3 April. An unambiguous value (day > 12) still parses
/// correctly either way.
const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",
    "%d/%m/%Y",
    "%d-%m-%Y",
    "%d.%m.%Y",
    "%d-%b-%Y",
    "%d %b %Y",
    "%d-%B-%Y",
    "%b %d, %Y",
    "%d/%m/%y",
    "%d-%m-%y",
    "%d.%m.%y",
    "%d-%b-%y",
    "%d %b %y",
    "%Y/%m/%d",
    "%m/%d/%Y",
];

/// A year this small is not one anyone traded in: it is `%Y` having
/// swallowed a two-digit year. `29-May-17` - how Value Research writes 29
/// May 2017 - matches `%d-%b-%Y` first, and chrono's `%Y` is content to read
/// `17` as the year 17. Rejecting that result lets the `%y` formats further
/// down the list have their turn; they widen 00-69 to the 2000s and 70-99
/// to the 1900s.
const MIN_PLAUSIBLE_YEAR: i32 = 1000;

pub fn parse_date(raw: &str) -> AppResult<NaiveDate> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AppError::bad("date is empty"));
    }

    // Excel hands back a serial number whenever the cell is date-formatted.
    if let Ok(serial) = s.parse::<f64>() {
        if serial > 1.0 && serial < 80_000.0 {
            if let Some(d) = excel_serial_to_date(serial) {
                return Ok(d);
            }
        }
    }

    // Some exports carry a time component; the date is all we keep.
    let date_part = s.split(['T', ' ']).next().unwrap_or(s);
    for fmt in DATE_FORMATS {
        for candidate in [s, date_part] {
            if let Ok(d) = NaiveDate::parse_from_str(candidate, fmt) {
                if d.year() >= MIN_PLAUSIBLE_YEAR {
                    return Ok(d);
                }
            }
        }
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.date());
    }
    let _ = NaiveTime::default();

    Err(AppError::bad(format!(
        "'{s}' is not a date we recognise - use YYYY-MM-DD or DD/MM/YYYY"
    )))
}

/// Parse a money or quantity cell.
///
/// Tolerates what spreadsheets actually contain: thousands separators
/// (including the Indian lakh grouping), currency symbols, a trailing or
/// leading sign, and accountants' parentheses for negatives.
pub fn parse_number(raw: &str) -> AppResult<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AppError::bad("number is empty"));
    }

    let negative = s.starts_with('(') && s.ends_with(')');
    let cleaned: String = s
        .trim_matches(|c| c == '(' || c == ')')
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();

    if cleaned.is_empty() || cleaned == "-" || cleaned == "." {
        return Err(AppError::bad(format!("'{s}' is not a number")));
    }

    let value: f64 = cleaned
        .parse()
        .map_err(|_| AppError::bad(format!("'{s}' is not a number")))?;

    Ok(if negative { -value.abs() } else { value })
}

/// Optional numeric cell: blank means "not supplied", not "zero-and-valid".
pub fn parse_number_opt(raw: Option<&str>) -> AppResult<Option<f64>> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => parse_number(s).map(Some),
    }
}

pub fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "true" | "1" | "t" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn normalises_messy_headers() {
        assert_eq!(normalise_header("Trade Date"), "trade_date");
        assert_eq!(normalise_header("  Qty.  "), "qty");
        assert_eq!(normalise_header("Price/Unit"), "price_unit");
        assert_eq!(normalise_header("NAV (Rs.)"), "nav_rs");
        assert_eq!(normalise_header("ISIN"), "isin");
    }

    #[test]
    fn parses_the_date_formats_indians_singaporeans_and_australians_write() {
        assert_eq!(parse_date("2026-09-18").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("18/09/2026").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("18-Sep-2026").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("18.09.2026").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("Sep 18, 2026").unwrap(), d(2026, 9, 18));
    }

    #[test]
    fn ambiguous_slash_dates_are_read_day_first() {
        // 3 April, not 4 March - the convention in all three markets.
        assert_eq!(parse_date("03/04/2026").unwrap(), d(2026, 4, 3));
    }

    #[test]
    fn two_digit_years_are_this_century_not_the_first() {
        // Value Research's statement writes 29-May-17. Read literally that
        // is the year 17, which is where every trade used to land.
        assert_eq!(parse_date("29-May-17").unwrap(), d(2017, 5, 29));
        assert_eq!(parse_date("03-Sep-26").unwrap(), d(2026, 9, 3));
        assert_eq!(parse_date("01-09-26").unwrap(), d(2026, 9, 1));
        assert_eq!(parse_date("03/04/26").unwrap(), d(2026, 4, 3));
        assert_eq!(parse_date("18.09.26").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("18 Sep 26").unwrap(), d(2026, 9, 18));
        // The pivot chrono itself uses: 70-99 are the 1900s.
        assert_eq!(parse_date("31-12-99").unwrap(), d(1999, 12, 31));
    }

    #[test]
    fn reads_excel_date_serials() {
        // 45000 is 2023-03-15 in Excel's 1900 system.
        assert_eq!(parse_date("45000").unwrap(), d(2023, 3, 15));
    }

    #[test]
    fn strips_timestamps() {
        assert_eq!(parse_date("2026-09-18 15:30:00").unwrap(), d(2026, 9, 18));
        assert_eq!(parse_date("2026-09-18T15:30:00Z").unwrap(), d(2026, 9, 18));
    }

    #[test]
    fn rejects_nonsense_dates_with_a_useful_message() {
        let err = parse_date("last tuesday").unwrap_err().to_string();
        assert!(err.contains("YYYY-MM-DD"), "got: {err}");
    }

    #[test]
    fn parses_numbers_with_separators_and_symbols() {
        assert_eq!(parse_number("1,234.56").unwrap(), 1234.56);
        assert_eq!(parse_number("\u{20B9} 1,23,456.78").unwrap(), 123456.78);
        assert_eq!(parse_number("$1,000").unwrap(), 1000.0);
        assert_eq!(parse_number("A$ 250.25").unwrap(), 250.25);
        assert_eq!(parse_number("  42  ").unwrap(), 42.0);
    }

    #[test]
    fn parses_negatives_both_ways() {
        assert_eq!(parse_number("-500").unwrap(), -500.0);
        assert_eq!(parse_number("(500)").unwrap(), -500.0);
        assert_eq!(parse_number("(1,250.50)").unwrap(), -1250.50);
    }

    #[test]
    fn blank_optional_numbers_stay_none() {
        assert_eq!(parse_number_opt(None).unwrap(), None);
        assert_eq!(parse_number_opt(Some("   ")).unwrap(), None);
        assert_eq!(parse_number_opt(Some("0")).unwrap(), Some(0.0));
    }

    #[test]
    fn reads_a_csv_into_normalised_rows() {
        let csv = "Trade Date,Symbol,Qty.,Price\n2026-01-15,INFY,10,1500.50\n";
        let sheet = read("x.csv", csv.as_bytes()).unwrap();
        assert_eq!(sheet.headers, vec!["trade_date", "symbol", "qty", "price"]);
        assert_eq!(sheet.rows.len(), 1);
        assert_eq!(sheet.rows[0].get("symbol"), Some("INFY"));
        assert_eq!(sheet.rows[0].number, 2);
    }

    #[test]
    fn blank_rows_are_dropped_not_reported_as_errors() {
        let csv = "date,symbol\n2026-01-15,INFY\n,\n2026-01-16,TCS\n";
        let sheet = read("x.csv", csv.as_bytes()).unwrap();
        assert_eq!(sheet.rows.len(), 2);
        // Row numbering still points at the real line in the file.
        assert_eq!(sheet.rows[1].number, 4);
    }

    #[test]
    fn unknown_extensions_are_refused_clearly() {
        let err = read("statement.pdf", b"x").unwrap_err().to_string();
        assert!(err.contains(".xlsx"), "got: {err}");
    }
}
