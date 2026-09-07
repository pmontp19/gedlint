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

/// Byte span of the URL inside a PLAC line: from "http" to the next
/// whitespace, or to the end of the line. The level is numeric and the tag is
/// PLAC, so the first "http" of the raw line is necessarily inside the value.
fn url_span(raw: &str) -> (u32, u32) {
    let Some(at) = raw.find("http") else { return (0, 0) };
    let rest = &raw[at..];
    let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
    (at as u32, len as u32)
}

/// W401 at level 1: URL inside PLAC (MyHeritage quirk).
pub(crate) fn check_plac_url_record(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && l.value.contains("http") {
        let (col, len) = url_span(&l.raw);
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
        let (col, len) = url_span(&l.raw);
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
