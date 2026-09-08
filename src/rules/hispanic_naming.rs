use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Line};
use crate::rules::names::surname_slot;

/// The defect W601 targets, as a pure predicate shared with the `--fix`
/// repair: exactly two non-empty tokens around one comma. More than two
/// tokens is not the export artifact this rule exists for, and the same
/// shape is the only thing the repair may rewrite.
pub(crate) fn comma_split(surname: &str) -> Option<(&str, &str)> {
    let mut parts = surname.split(',');
    let a = parts.next().unwrap_or("");
    let b = parts.next().unwrap_or("");
    if parts.next().is_some() || a.trim().is_empty() || b.trim().is_empty() {
        return None;
    }
    Some((a.trim(), b.trim()))
}

/// W601: no-comma-in-surname, on the whole `1 NAME` value (the surname is
/// read out of the slash slot). A `2 SURN` subtag goes through
/// [`check_surname_value`]. Returns whether it fired.
pub(crate) fn check_surname_comma(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    match surname_slot(val) {
        Some(surname) => check_surname_value(diags, line, surname),
        None => false,
    }
}

/// W601 on a bare surname string: the value of a `2 SURN` subtag, or the
/// slot cut out of a `1 NAME`. Returns whether it fired.
pub(crate) fn check_surname_value(diags: &mut Vec<Diag>, line: usize, surname: &str) -> bool {
    if comma_split(surname).is_none() {
        return false;
    }
    diags.push(
        Diag::new(
            "W601",
            Category::Style,
            Severity::Warning,
            line,
            format!("comma in surname: {}", truncate(surname, 60)),
        )
        .in_ruleset("hispanic-naming"),
    );
    true
}

/// W602: no-married-name
pub(crate) fn check_married_name(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "_MARNM" {
        diags.push(
            Diag::new(
                "W602",
                Category::Style,
                Severity::Warning,
                l.no,
                "married name tag (_MARNM) is usually an export artifact".to_string(),
            )
            .in_ruleset("hispanic-naming"),
        );
    }
}

/// W603: no-abbreviated-given-name, on the whole `1 NAME` value or the
/// value of a `2 GIVN` subtag. Returns whether it fired.
pub(crate) fn check_abbreviated_given_name(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    let bad = ["Mª", "Ma.", "Fco.", "Jph."];
    for b in bad.iter() {
        if val.contains(b) {
            diags.push(
                Diag::new(
                    "W603",
                    Category::Style,
                    Severity::Info,
                    line,
                    format!("abbreviated given name: {}", b),
                )
                .in_ruleset("hispanic-naming"),
            );
            return true;
        }
    }
    false
}
