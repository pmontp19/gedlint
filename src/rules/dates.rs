//! W402/U501 on DATE payloads: month spelling, approximations, ranges,
//! parentheses and the calendar escape.

use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Version};

pub(crate) fn check_date_style(diags: &mut Vec<Diag>, line: usize, value: &str, version: Version) {
    let v = value.trim();
    // Months in other languages or lowercase: GEDCOM requires JAN FEB MAR...
    let lower_months = ["enero", "febrero", "gener", "febrer", "marzo", "març", "abril", "mayo", "maig", "junio", "juny"];
    let vl = v.to_lowercase();
    if lower_months.iter().any(|m| vl.contains(m)) {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!("DATE with non-standard month (use JAN/FEB/...): {}", truncate(v, 50))
        ));
        return;
    }
    if v.starts_with("about") || v.starts_with("circa") || v.starts_with("aprox") {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!("DATE with lowercase approximation (use ABT/CAL/EST): {}", truncate(v, 50))
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
                format!("BET without AND (7.0 needs a full range): {}", truncate(v, 50))
            ));
        } else {
            diags.push(Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("BET without AND (DATE_RANGE needs BET x AND y): {}", truncate(v, 50))
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
                format!("BET range out of order (swap to chronological): {}", truncate(v, 50))
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
            format!("DATE with unbalanced parentheses: {}", truncate(v, 50))
        ));
    }
    // Calendar escape @#...@: must close and name a known calendar.
    if let Some(start) = v.find("@#") {
        const CALENDARS: &[&str] = &["GREGORIAN", "JULIAN", "HEBREW", "FRENCH_R", "ROMAN", "UNKNOWN"];
        let rest = &v[start + 2..];
        match rest.find('@') {
            Some(end) if CALENDARS.contains(&rest[..end].to_ascii_uppercase().as_str()) => {}
            _ => diags.push(Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE with bad calendar escape (use @#GREGORIAN@ etc.): {}", truncate(v, 50))
            )),
        }
    }
}
