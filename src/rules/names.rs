//! NAME accumulation across CONC/CONT and the W402 slash-balance check.
//!
//! W402 must see the whole NAME value: a surname split across CONC lines
//! (Long26CC) is balanced as a whole but odd per line. The hispanic-naming
//! and hygiene checks run here too, on the accumulated value at flush time,
//! and on the SURN/GIVN subtags ([`check_subtag`]) with one diagnostic per
//! record per rule.

use std::collections::HashMap;

use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Line};

use super::hispanic_naming;
use super::hygiene;

/// The NAME run currently open: (xref, line, accumulated value).
#[derive(Default)]
pub(crate) struct Names {
    pub(crate) name_buf: Option<(String, usize, String)>,
    /// Rules that already produced a diagnostic for the current record.
    /// A surname defect usually sits in both `1 NAME` and `2 SURN`, because
    /// exports write both, and that is one defect, not two: whichever field
    /// is checked first wins (the NAME run flushes before its subtags are
    /// read) and every later field for the same record stays silent.
    /// Cleared when a new record opens.
    pub(crate) reported: Vec<&'static str>,
}

/// The surname slot of a NAME value: between the first pair of slashes.
/// Shared by W601 and W702, which both mean "this surname string".
pub(crate) fn surname_slot(val: &str) -> Option<&str> {
    let start = val.find('/')? + 1;
    let end = val[start..].find('/')? + start;
    Some(&val[start..end])
}

/// W402 check on the accumulated NAME value (NAME + CONC/CONT run), then
/// the rulesets' name checks, recording what fired.
pub(crate) fn flush(
    diags: &mut Vec<Diag>,
    st: &mut Names,
    names: &mut HashMap<String, String>,
) {
    let Some((xref, line, val)) = st.name_buf.take() else {
        return;
    };
    names.insert(xref.clone(), val.clone());
    if val.matches('/').count() % 2 != 0 {
        diags.push(Diag::new(
            "W402",
            Category::Style,
            Severity::Warning,
            line,
            format!("{}: NAME with unbalanced slashes: {}", xref, truncate(&val, 50)),
        ));
    }

    if hispanic_naming::check_surname_comma(diags, line, &val) {
        st.reported.push("W601");
    }
    if hispanic_naming::check_abbreviated_given_name(diags, line, &val) {
        st.reported.push("W603");
    }
    if hygiene::check_polluted_name(diags, line, &val) {
        st.reported.push("W701");
    }
    if hygiene::check_all_caps_name(diags, line, &val) {
        st.reported.push("W702");
    }
}

/// A level-1 INDI.NAME opens a run; the value may continue via CONC/CONT, so
/// W402 runs on the whole accumulated value at flush time.
pub(crate) fn open(st: &mut Names, l: &Line, xref: &str) {
    st.name_buf = Some((xref.to_string(), l.no, l.value.clone()));
}

/// Level >= 2: append CONC verbatim and CONT after a space. The run is only
/// open while CONC/CONT directly follow the NAME at level 2: any other line
/// (a SOUR between, or a CONC deeper down that belongs to a substructure like
/// SOUR.PAGE) closes it first so foreign values are never absorbed.
pub(crate) fn continue_value(
    diags: &mut Vec<Diag>,
    st: &mut Names,
    names: &mut HashMap<String, String>,
    l: &Line,
    lvl: u32,
) {
    if st.name_buf.is_some() {
        if lvl == 2 && (l.tag == "CONC" || l.tag == "CONT") {
            if let Some((_, _, val)) = &mut st.name_buf {
                if l.tag == "CONC" {
                    val.push_str(&l.value);
                } else {
                    val.push(' ');
                    val.push_str(&l.value);
                }
            }
        } else {
            flush(diags, st, names);
        }
    }
}

/// One field, one rule: the check runs only when this record has not
/// already reported the rule, and a fire records the code. The `reported`
/// guard is consulted *before* the check, so a suppressed defect never
/// produces a diagnostic at all. Kept out of the match arm body in
/// [`check_subtag`] so no `if` nests inside an arm, for any code.
fn check_once(
    diags: &mut Vec<Diag>,
    st: &mut Names,
    code: &'static str,
    fires: impl FnOnce(&mut Vec<Diag>) -> bool,
) {
    if st.reported.contains(&code) {
        return;
    }
    if fires(diags) {
        st.reported.push(code);
    }
}

/// SURN and GIVN subtags under a NAME line: the structured fields the two
/// rulesets' specifications name alongside `1 NAME` itself. Runs after
/// [`flush`], so `reported` already knows what the NAME value produced and
/// a defect sitting in both places is reported once, not twice.
pub(crate) fn check_subtag(diags: &mut Vec<Diag>, st: &mut Names, tag: &str, val: &str, line: usize) {
    if val.is_empty() {
        return;
    }
    match tag {
        "SURN" => {
            check_once(diags, st, "W601", |d| hispanic_naming::check_surname_value(d, line, val));
            check_once(diags, st, "W701", |d| hygiene::check_polluted_name(d, line, val));
            check_once(diags, st, "W702", |d| hygiene::check_all_caps_value(d, line, val));
        }
        "GIVN" => {
            check_once(diags, st, "W603", |d| hispanic_naming::check_abbreviated_given_name(d, line, val));
        }
        _ => {}
    }
}
