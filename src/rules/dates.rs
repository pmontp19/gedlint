//! W402/U501 on DATE payloads: month spelling, approximations, ranges,
//! parentheses and the calendar escape.

use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Version};

pub(crate) fn check_date_style(diags: &mut Vec<Diag>, line: usize, value: &str, version: Version) {
    let v = value.trim();
    // Months in other languages or lowercase: GEDCOM requires JAN FEB MAR...
    let lower_months = [
        "enero", "febrero", "gener", "febrer", "marzo", "març", "abril", "mayo", "maig", "junio",
        "juny",
    ];
    let vl = v.to_lowercase();
    if lower_months.iter().any(|m| vl.contains(m)) {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!(
                "DATE with non-standard month (use JAN/FEB/...): {}",
                truncate(v, 50)
            ),
        ));
        return;
    }
    if v.starts_with("about") || v.starts_with("circa") || v.starts_with("aprox") {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!(
                "DATE with lowercase approximation (use ABT/CAL/EST): {}",
                truncate(v, 50)
            ),
        ));
    }
    if v.contains("BET") && !v.contains("AND") {
        // DATE_RANGE needs BET x AND y in both 5.5.1 and 7.0.
        if version == Version::V70 {
            diags.push(Diag::new(
                "U501",
                Category::Upgrade,
                Severity::Info,
                line,
                format!(
                    "BET without AND (7.0 needs a full range): {}",
                    truncate(v, 50)
                ),
            ));
        } else {
            diags.push(Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!(
                    "BET without AND (DATE_RANGE needs BET x AND y): {}",
                    truncate(v, 50)
                ),
            ));
        }
    }
    if v.contains("BET") && v.contains("AND") {
        // 7.0 ranges must be chronological (migrate guide: swap if needed).
        let years: Vec<i64> = v
            .split(|c: char| !c.is_ascii_digit())
            .filter(|t| t.len() == 4)
            .filter_map(|t| t.parse().ok())
            .collect();
        if years.len() >= 2 && years[0] > years[1] {
            diags.push(Diag::new(
                "U501",
                Category::Upgrade,
                Severity::Info,
                line,
                format!(
                    "BET range out of order (swap to chronological): {}",
                    truncate(v, 50)
                ),
            ));
        }
    }
    // FROM/TO pairing is NOT checked: DATE_PERIOD allows each half alone in
    // both 5.5.1 (p.43) and 7.0 (TGC551LF uses standalone FROM and TO).
    // Balanced parentheses (DATE_PHRASE).
    if v.matches('(').count() != v.matches(')').count() {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!("DATE with unbalanced parentheses: {}", truncate(v, 50)),
        ));
    }
    // Calendar escape @#...@: must close and name a known calendar.
    if let Some(start) = v.find("@#") {
        const CALENDARS: &[&str] = &[
            "GREGORIAN",
            "JULIAN",
            "HEBREW",
            "FRENCH_R",
            "ROMAN",
            "UNKNOWN",
        ];
        let rest = &v[start + 2..];
        match rest.find('@') {
            Some(end) if CALENDARS.contains(&rest[..end].to_ascii_uppercase().as_str()) => {}
            _ => diags.push(Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!(
                    "DATE with bad calendar escape (use @#GREGORIAN@ etc.): {}",
                    truncate(v, 50)
                ),
            )),
        }
    }
}

/// Exact Gregorian day ordinal for a DATE value, or `None` when the value
/// is not an exact day-month-year date. Pure integer arithmetic, no
/// `std::time`, no dependencies: safe on `wasm32-unknown-unknown`.
///
/// Only `DD MMM YYYY` with English month abbreviations counts. Year-only,
/// month-only, ranges (`BET`, `FROM`, `TO`), approximations (`ABT`, `CAL`,
/// `EST`, `BEF`, `AFT`, `INT`) and calendar escapes yield `None`, so W704
/// only measures spacing it can actually know.
pub(crate) fn date_ordinal(s: &str) -> Option<i64> {
    let up = s.to_ascii_uppercase();
    for q in [
        "BEF", "AFT", "ABT", "CAL", "EST", "BET", "AND", "FROM", "TO", "INT", "@#",
    ] {
        if up.split(|c: char| !c.is_ascii_alphabetic()).any(|t| t == q) {
            return None;
        }
        if q == "@#" && up.contains("@#") {
            return None;
        }
    }
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mi = toks.iter().position(|t| month_num(t).is_some())?;
    let m = month_num(toks[mi])?;
    let d: i64 = toks.get(mi.wrapping_sub(1)).and_then(|t| t.parse().ok())?;
    let y: i64 = toks.get(mi + 1).and_then(|t| t.parse().ok())?;
    if !(100..=2100).contains(&y) || !(1..=31).contains(&d) {
        return None;
    }
    if d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn month_num(t: &str) -> Option<i64> {
    match t.to_ascii_uppercase().as_str() {
        "JAN" => Some(1),
        "FEB" => Some(2),
        "MAR" => Some(3),
        "APR" => Some(4),
        "MAY" => Some(5),
        "JUN" => Some(6),
        "JUL" => Some(7),
        "AUG" => Some(8),
        "SEP" => Some(9),
        "OCT" => Some(10),
        "NOV" => Some(11),
        "DEC" => Some(12),
        _ => None,
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Howard Hinnant's days-from-civil, proleptic Gregorian. The epoch is
/// arbitrary (1970-01-01 = day 0 here would also do): only differences
/// matter for sibling spacing.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let mut y = y;
    let m_adj = m;
    y -= i64::from(m_adj <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m_adj + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
