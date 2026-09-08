use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Line};
use crate::rules::names::surname_slot;

/// W701: polluted-name, on any raw name value: the whole `1 NAME` value or
/// the value of a `2 SURN` subtag. Returns whether it fired.
pub(crate) fn check_polluted_name(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    let mut polluted = false;
    if val.contains('*')
        || val.chars().any(|c| c.is_ascii_digit())
        || val.contains('º')
        || val.contains('ª')
    {
        polluted = true;
    } else if val.contains('(') {
        let lower = val.to_lowercase();
        // Conservative: a parenthetical can be a legitimate Catalan house name
        if !lower.contains("(cal ")
            && !lower.contains("(can ")
            && !lower.contains("(mas ")
            && !lower.contains("(casa ")
            && !lower.contains("(de ")
        {
            polluted = true;
        }
    }

    if polluted {
        diags.push(
            Diag::new(
                "W701",
                Category::Style,
                Severity::Info,
                line,
                format!("polluted name field: {}", truncate(val, 60)),
            )
            .in_ruleset("hygiene"),
        );
        return true;
    }
    false
}

/// W702: all-caps-name, on the whole `1 NAME` value (the surname is read
/// out of the slash slot). A `2 SURN` subtag goes through
/// [`check_all_caps_value`]. Returns whether it fired.
pub(crate) fn check_all_caps_name(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    match surname_slot(val) {
        Some(surname) => check_all_caps_value(diags, line, surname),
        None => false,
    }
}

/// The defect W702 targets, as a pure predicate shared with the `--fix`
/// repair: letters present, no lowercase among them.
pub(crate) fn is_all_caps(surname: &str) -> bool {
    let has_letters = surname.chars().any(|c| c.is_alphabetic());
    let has_lower = surname.chars().any(|c| c.is_lowercase());
    has_letters && !has_lower
}

/// W702 on a bare surname string: the value of a `2 SURN` subtag, or the
/// slot cut out of a `1 NAME`. Returns whether it fired.
pub(crate) fn check_all_caps_value(diags: &mut Vec<Diag>, line: usize, surname: &str) -> bool {
    if !is_all_caps(surname) {
        return false;
    }
    diags.push(
        Diag::new(
            "W702",
            Category::Style,
            Severity::Info,
            line,
            format!("all-caps surname: {}", truncate(surname, 60)),
        )
        .in_ruleset("hygiene"),
    );
    true
}

/// W703: malformed-place
pub(crate) fn check_malformed_place(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && (l.value.contains(",,") || l.value.contains(", ,")) {
        diags.push(
            Diag::new(
                "W703",
                Category::Style,
                Severity::Info,
                l.no,
                format!("doubled commas in place: {}", truncate(&l.value, 60)),
            )
            .in_ruleset("hygiene"),
        );
    }
}
