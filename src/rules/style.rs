//! Exporter-quirk style rules: control characters (W102), URLs inside PLAC
//! (W401) and HTML inside NOTE (W403).

use crate::diag::{push_capped, Category, Diag, Severity};
use crate::parse::{truncate, Line};

/// W102: ASCII controls (other than \t).
pub(crate) fn check_control_chars(diags: &mut Vec<Diag>, l: &Line) {
    if l.raw.chars().any(|c| c.is_control() && c != '\t') {
        push_capped(
            diags,
            vec![Diag::new(
                "W102",
                Category::Style,
                Severity::Warning,
                l.no,
                "control character inside line".into(),
            )],
        );
    }
}

/// Byte span of the URL inside a PLAC line: the first value token that
/// *starts* with "http", from there to the next whitespace or to the end of
/// the line. Anchored on a token boundary and searched inside the value only,
/// so neither the tag nor a decoy like "chttpx" can capture the span. No
/// token starts with "http" (the rule fires on a bare substring match, so
/// that is possible): no span, and the whole line is highlighted instead.
fn url_span(l: &Line) -> (u32, u32) {
    let at = l
        .value
        .match_indices("http")
        .find(|(i, _)| *i == 0 || l.value[..*i].ends_with(char::is_whitespace))
        .map(|(i, _)| i);
    let Some(at) = at else { return (0, 0) };
    let rest = &l.value[at..];
    let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
    l.span_at(l.value_col + at, len)
}

/// W401 at level 1: URL inside PLAC (MyHeritage quirk).
pub(crate) fn check_plac_url_record(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && l.value.contains("http") {
        let (col, len) = url_span(l);
        push_capped(
            diags,
            vec![Diag::with_span(
                "W401",
                Category::Style,
                Severity::Warning,
                l.no,
                col,
                len,
                format!("PLAC with URL (MyHeritage quirk): move it to NOTE: {}", truncate(&l.value, 60)),
            )],
        );
    }
}

/// W403: HTML notes inside NOTE.
pub(crate) fn check_note_html(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "NOTE" && (l.value.contains("<br") || l.value.contains("<notexml") || l.value.contains("&nbsp")) {
        push_capped(
            diags,
            vec![Diag::new(
                "W403",
                Category::Style,
                Severity::Warning,
                l.no,
                format!("NOTE with HTML (exporter quirk): {}", truncate(&l.value, 60)),
            )],
        );
    }
}

/// W401 below level 1: nested PLAC with URL (MyHeritage quirk).
pub(crate) fn check_plac_url(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && l.value.contains("http") {
        let (col, len) = url_span(l);
        push_capped(
            diags,
            vec![Diag::with_span(
                "W401",
                Category::Style,
                Severity::Warning,
                l.no,
                col,
                len,
                format!("PLAC with URL (MyHeritage quirk): {}", truncate(&l.value, 60)),
            )],
        );
    }
}
