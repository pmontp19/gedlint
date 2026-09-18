//! Payload grammar checks that sit outside any single tag family: the AGE
//! duration form (W404) and the 5.5.1 calendar escapes on DATE (W405).

use crate::diag::{Category, Diag, Severity};
use crate::parse::Version;

// 5.5.1 spells three whole-word ages the 7.0 grammar dropped.
const AGE_WORDS551: &[&str] = &["INFANT", "CHILD", "STILLBORN"];

/// W404: AGE carries a duration like "42y 6m", with an optional < or >
/// bound, or one of the three 5.5.1 words. A bare number or spelled-out
/// units ("76", "3 months") is stored as unparsed text: it never lands on
/// an age computation or a timeline.
pub(crate) fn check_age(diags: &mut Vec<Diag>, line: usize, value: &str, version: Version) {
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    if version != Version::V70 && AGE_WORDS551.contains(&v.to_ascii_uppercase().as_str()) {
        return;
    }
    if age_duration_ok(v) {
        return;
    }
    let hint = if version == Version::V70 {
        ""
    } else {
        " or INFANT/CHILD/STILLBORN"
    };
    diags.push(Diag::new(
        "W404",
        Category::Style,
        Severity::Warning,
        line,
        format!(
            "malformed AGE value {:?} (write a duration like 42y 6m, bounds like >70y allowed{})",
            truncate_value(v),
            hint
        ),
    ));
}

/// The 7.0 Age production, liberal about unit case: one optional < or >
/// bound, then one to four `digits + unit` tokens in y -> m -> w -> d order,
/// each unit once. Every token needs its unit: "76" is not an age.
fn age_duration_ok(v: &str) -> bool {
    let mut s = v;
    if let Some(rest) = s.strip_prefix('<').or_else(|| s.strip_prefix('>')) {
        s = rest.trim_start();
    }
    let mut last = 0u8;
    let mut any = false;
    for tok in s.split_whitespace() {
        let Some((num, unit)) = tok.as_bytes().split_last_chunk::<1>() else {
            return false;
        };
        let order = match unit {
            b"y" | b"Y" => 1,
            b"m" | b"M" => 2,
            b"w" | b"W" => 3,
            b"d" | b"D" => 4,
            _ => return false,
        };
        if order <= last
            || num.is_empty()
            || !num.iter().all(|b| b.is_ascii_digit())
            || num.len() > 3
        {
            return false;
        }
        last = order;
        any = true;
    }
    any
}

// The 5.5.1 Hebrew and French Republican month codes. None of them
// collides with a Gregorian month or an event tag.
const HEBREW_MONTHS: &[&str] = &[
    "TSH", "CSH", "KSL", "TVT", "SHV", "ADR", "ADS", "NSN", "IYR", "SVN", "TMZ", "AAV", "ELL",
];
const FRENCH_MONTHS: &[&str] = &[
    "VEND", "BRUM", "FRIM", "NIVO", "PLUV", "VENT", "GERM", "FLOR", "PRAI", "MESS", "THER", "FRUC",
    "COMP",
];

/// W405: a 5.5.1 date in the Hebrew or French Republican calendar needs its
/// `@#DHEBREW@` / `@#DFRENCH R@` escape prefix. Without it the month code is
/// not a month at all: importers store the value as unparsed text or refuse
/// the date (ged-inline.org flags every bare Hebrew/French date in TGC551).
/// 7.0 names calendars inline, so it is exempt.
pub(crate) fn check_calendar_escape(
    diags: &mut Vec<Diag>,
    line: usize,
    value: &str,
    version: Version,
) {
    if version == Version::V70 {
        return;
    }
    let v = value.trim();
    if v.is_empty() || v.contains("@#D") {
        return;
    }
    let month = v
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|t| HEBREW_MONTHS.contains(t) || FRENCH_MONTHS.contains(t));
    let Some(month) = month else { return };
    let calendar = if HEBREW_MONTHS.contains(&month) {
        "@#DHEBREW@"
    } else {
        "@#DFRENCH R@"
    };
    diags.push(Diag::new(
        "W405",
        Category::Style,
        Severity::Warning,
        line,
        format!(
            "date month {month} needs the {calendar} escape prefix in 5.5.1 (non-Gregorian dates are only readable with it)"
        ),
    ));
}

fn truncate_value(v: &str) -> String {
    let mut out: String = v.chars().take(40).collect();
    if out.len() < v.len() {
        out.push('…');
    }
    out
}
