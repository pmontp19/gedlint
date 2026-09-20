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
    if age_duration_ok(v, version) {
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
            "malformed AGE value {:?} (write a duration like 42y 6m; bounds are written like > 70y{})",
            truncate_value(v),
            hint
        ),
    ));
}

/// The age-duration grammar of the detected version: an optional < or >
/// bound, then one to four `digits + unit` tokens in y -> m -> w -> d order,
/// each unit once. Every token needs its unit: "76" is not an age. Weeks
/// were introduced in 7.0 (5.5.1 has y/m/d only), 7.0 requires the space
/// after a bound ("-ageBound D"), and no version caps the digits.
fn age_duration_ok(v: &str, version: Version) -> bool {
    let mut s = v;
    if let Some(rest) = s.strip_prefix('<').or_else(|| s.strip_prefix('>')) {
        if version == Version::V70 {
            // 7.0: the bound is its own component, delimited by one space.
            let Some(rest) = rest.strip_prefix(' ') else {
                return false;
            };
            s = rest;
        } else {
            s = rest.trim_start();
        }
    }
    let mut last = 0u8;
    let mut any = false;
    let tokens: Box<dyn Iterator<Item = &str>> = if version == Version::V70 {
        // One space between components: an empty token means a doubled one.
        Box::new(s.split(' '))
    } else {
        Box::new(s.split_whitespace())
    };
    for tok in tokens {
        let Some((num, unit)) = tok.as_bytes().split_last_chunk::<1>() else {
            return false;
        };
        let order = match unit {
            b"y" | b"Y" => 1,
            b"m" | b"M" => 2,
            b"w" | b"W" if version != Version::V551 => 3,
            b"d" | b"D" => 4,
            _ => return false,
        };
        if order <= last || num.is_empty() || !num.iter().all(|b| b.is_ascii_digit()) {
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
/// `@#DHEBREW@` / `@#DFRENCH R@` escape prefix. Gated on the proven 5.5.1
/// version like E010: a file whose VERS is missing already reports E009 for
/// that, and 7.0 names calendars inline.
pub(crate) fn check_calendar_escape(
    diags: &mut Vec<Diag>,
    line: usize,
    value: &str,
    version: Version,
) {
    if version != Version::V551 {
        return;
    }
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    for component in date_components(v) {
        let c = component.trim();
        if c.is_empty() {
            continue;
        }
        let Some(month) = c.split(|c: char| !c.is_ascii_alphanumeric()).find_map(|t| {
            let upper = t.to_ascii_uppercase();
            if HEBREW_MONTHS.contains(&upper.as_str()) || FRENCH_MONTHS.contains(&upper.as_str()) {
                Some(upper)
            } else {
                None
            }
        }) else {
            continue;
        };
        let calendar = if HEBREW_MONTHS.contains(&month.as_str()) {
            "@#DHEBREW@"
        } else {
            "@#DFRENCH R@"
        };
        // The escape must open the component it covers: an escape for a
        // different calendar or one buried mid-value does not exempt the
        // date.
        if c.starts_with(calendar) {
            continue;
        }
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
}

/// One GEDCOM 5.5.1 DATE value can hold several date components separated
/// by the range/approximation keywords. Split on those keywords so each
/// component's calendar escape is judged on its own half.
fn date_components(v: &str) -> Vec<String> {
    const KEYWORDS: &[&str] = &[
        "FROM", "TO", "BET", "AND", "BEF", "AFT", "ABT", "CAL", "EST", "INT",
    ];
    let mut parts: Vec<String> = vec![String::new()];
    for word in v.split_whitespace() {
        // Keywords compare case-insensitively: a lowercase "from" still
        // opens a new date component, and its months still need escapes.
        if KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word)) {
            parts.push(String::new());
        } else {
            let last = parts.last_mut().unwrap();
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    parts
}

fn truncate_value(v: &str) -> String {
    let mut out: String = v.chars().take(40).collect();
    if out.len() < v.len() {
        out.push('…');
    }
    out
}
